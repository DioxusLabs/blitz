use quadtree_rs::area::AreaBuilder;
use quadtree_rs::Quadtree;
use rustc_hash::FxHashSet;
use shipyard::Component;
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockWriteGuard};
use taffy::geometry::Point;
use taffy::prelude::Layout;
use tao::{dpi::PhysicalSize, event_loop::EventLoopProxy, window::Window};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use vello::{
    peniko::Color,
    util::{RenderContext, RenderSurface},
    RenderParams, Scene, SceneBuilder,
};
use vello::{Renderer as VelloRenderer, RendererOptions};

use crate::{
    events::{BlitzEventHandler, DomEvent},
    focus::{Focus, FocusState},
    image::LoadedImage,
    layout::TaffyLayout,
    mouse::MouseEffected,
    prevent_default::PreventDefault,
    render::render,
    style::{BackgroundColor, Border, FontSize, ForgroundColor},
    text::TextContext,
    Redraw, TaoEvent,
};
use crate::{image::ImageContext, Driver};
use dioxus_native_core::{prelude::*, FxDashSet};
use taffy::{
    prelude::{AvailableSpace, Size},
    style::Dimension,
    Taffy,
};

pub struct ApplicationState {
    dom: DomManager,
    text_context: TextContext,
    render_context: RenderContext,
    surface: RenderSurface,
    wgpu_renderer: VelloRenderer,
    event_handler: BlitzEventHandler,
    quadtree: Quadtree<u64, NodeId>,
}

impl ApplicationState {
    /// Create a new window state and spawn a vdom thread.
    pub async fn new<R: Driver>(
        spawn_renderer: impl FnOnce(&Arc<RwLock<RealDom>>, &Arc<Mutex<Taffy>>) -> R + Send + 'static,
        window: &Window,
        proxy: EventLoopProxy<Redraw>,
    ) -> Self {
        let inner_size = window.inner_size();

        let mut rdom = RealDom::new([
            MouseEffected::to_type_erased(),
            TaffyLayout::to_type_erased(),
            ForgroundColor::to_type_erased(),
            BackgroundColor::to_type_erased(),
            Border::to_type_erased(),
            Focus::to_type_erased(),
            PreventDefault::to_type_erased(),
            LoadedImage::to_type_erased(),
            FontSize::to_type_erased(),
        ]);

        let focus_state = FocusState::create(&mut rdom);

        let dom = DomManager::spawn(rdom, inner_size, spawn_renderer, proxy);

        let event_handler = BlitzEventHandler::new(focus_state);

        let mut render_context = RenderContext::new().unwrap();
        let size = window.inner_size();
        let surface = render_context
            .create_surface(window, size.width, size.height)
            .await;
        let wgpu_renderer = VelloRenderer::new(
            &render_context.devices[surface.dev_id].device,
            &RendererOptions {
                surface_format: Some(surface.config.format),
            },
        )
        .unwrap();

        let text_context = TextContext::default();

        ApplicationState {
            dom,
            text_context,
            render_context,
            wgpu_renderer,
            surface,
            event_handler,
            quadtree: Quadtree::new(20),
        }
    }

    pub fn render(&mut self) {
        let mut scene = Scene::new();
        let mut builder = SceneBuilder::for_scene(&mut scene);
        self.dom.render(&mut self.text_context, &mut builder);
        // builder.finish();
        let surface_texture = self
            .surface
            .surface
            .get_current_texture()
            .expect("failed to get surface texture");
        let device = &self.render_context.devices[self.surface.dev_id];
        self.wgpu_renderer
            .render_to_surface(
                &device.device,
                &device.queue,
                &scene,
                &surface_texture,
                &RenderParams {
                    base_color: Color::WHITE,
                    width: self.surface.config.width,
                    height: self.surface.config.height,
                },
            )
            .expect("failed to render to surface");
        surface_texture.present();
        device.device.poll(wgpu::Maintain::Wait);

        // After we render, we need to update the quadtree to reflect the new positions of the nodes
        self.update_quadtree();
    }

    // TODO: Once we implement a custom tree for Taffy we can call this when the layout actually changes for each node instead of the diffing approach this currently uses
    fn update_quadtree(&mut self) {
        #[derive(Component)]
        struct QuadtreeId(u64);

        fn add_to_quadtree(
            node_id: NodeId,
            parent_location: Point<f32>,
            taffy: &Taffy,
            rdom: &mut RealDom,
            quadtree: &mut Quadtree<u64, NodeId>,
        ) {
            if let Some(node) = rdom.get(node_id) {
                if let Some((size, location)) = {
                    let layout = node.get::<TaffyLayout>();
                    layout.and_then(|l| {
                        if let Ok(Layout { size, location, .. }) = taffy.layout(l.node.unwrap()) {
                            Some((size, location))
                        } else {
                            None
                        }
                    })
                } {
                    let location = Point {
                        x: location.x + parent_location.x,
                        y: location.y + parent_location.y,
                    };

                    let mut qtree_id = None;
                    let area = AreaBuilder::default()
                        .anchor((location.x as u64, location.y as u64).into())
                        .dimensions((size.width as u64, size.height as u64))
                        .build()
                        .unwrap();
                    match node.get::<QuadtreeId>() {
                        Some(id) => {
                            let id = id.0;
                            if let Some(entry) = quadtree.get(id) {
                                let old_area = entry.area();
                                // If the area has changed, we need to update the quadtree
                                if old_area != area {
                                    quadtree.delete_by_handle(id);
                                    qtree_id = quadtree.insert(area, node_id);
                                }
                            } else {
                                // If the node is not in the quadtree, we need to add it
                                qtree_id = quadtree.insert(area, node_id);
                            }
                        }
                        None => {
                            // If the node is not in the quadtree, we need to add it
                            qtree_id = quadtree.insert(area, node_id);
                        }
                    }
                    // Repeat for all children
                    for child in node.child_ids() {
                        add_to_quadtree(child, location, taffy, rdom, quadtree);
                    }
                    // If the node was added or updated, we need to update the node's quadtree id
                    if let Some(id) = qtree_id {
                        let mut node = rdom.get_mut(node_id).unwrap();
                        node.insert(QuadtreeId(id));
                    }
                }
            }
        }
        let mut rdom = self.dom.rdom();
        let taffy = self.dom.taffy();
        add_to_quadtree(
            rdom.root_id(),
            Point::ZERO,
            &taffy,
            &mut rdom,
            &mut self.quadtree,
        );
    }

    pub fn set_size(&mut self, size: PhysicalSize<u32>) {
        // the window size is zero when minimized which causes the renderer to panic
        if size.width > 0 && size.height > 0 {
            self.dom.set_size(size);
            self.render_context
                .resize_surface(&mut self.surface, size.width, size.height);
        }
    }

    pub fn clean(&mut self) -> DirtyNodes {
        self.event_handler.clean().or(self.dom.clean())
    }

    pub fn send_event(&mut self, event: &TaoEvent) {
        let size = self.dom.size();
        let size = Size {
            width: size.width,
            height: size.height,
        };
        let evts;
        {
            let rdom = &mut self.dom.rdom();
            let taffy = &self.dom.taffy();
            self.event_handler
                .register_event(event, rdom, taffy, &size, &self.quadtree);
            evts = self.event_handler.drain_events();
        }
        self.dom.send_events(evts);
    }
}

#[allow(clippy::too_many_arguments)]
async fn spawn_dom<R: Driver>(
    rdom: Arc<RwLock<RealDom>>,
    taffy: Arc<Mutex<Taffy>>,
    size: Arc<Mutex<PhysicalSize<u32>>>,
    spawn_renderer: impl FnOnce(&Arc<RwLock<RealDom>>, &Arc<Mutex<Taffy>>) -> R,
    proxy: EventLoopProxy<Redraw>,
    mut event_receiver: UnboundedReceiver<DomEvent>,
    mut redraw_receiver: UnboundedReceiver<()>,
    vdom_dirty: Arc<FxDashSet<NodeId>>,
) -> Option<()> {
    let text_context = Arc::new(Mutex::new(TextContext::default()));
    let mut renderer = spawn_renderer(&rdom, &taffy);
    let mut last_size;
    let image_context = ImageContext::default();

    // initial render
    {
        let mut rdom = rdom.write().ok()?;
        let root_id = rdom.root_id();
        renderer.update(rdom.get_mut(root_id)?);
        let mut ctx = SendAnyMap::new();
        ctx.insert(taffy.clone());
        ctx.insert(image_context.clone());
        ctx.insert(text_context.clone());
        // update the state of the real dom
        let (to_rerender, _) = rdom.update_state(ctx);
        let size = size.lock().unwrap();

        let width = size.width as f32;
        let height = size.height as f32;
        let size = Size {
            width: AvailableSpace::Definite(width),
            height: AvailableSpace::Definite(height),
        };

        last_size = size;

        let mut locked_taffy = taffy.lock().unwrap();

        // the root node fills the entire area
        let root_node = rdom.get(rdom.root_id()).unwrap();
        let root_taffy_node = root_node.get::<TaffyLayout>().unwrap().node.unwrap();

        let mut style = locked_taffy.style(root_taffy_node).unwrap().clone();
        style.size = Size {
            width: Dimension::Points(width),
            height: Dimension::Points(height),
        };
        locked_taffy.set_style(root_taffy_node, style).unwrap();
        locked_taffy.compute_layout(root_taffy_node, size).unwrap();
        for k in to_rerender.into_iter() {
            vdom_dirty.insert(k);
        }
        proxy.send_event(Redraw).unwrap();
    }

    loop {
        let wait = renderer.poll_async();
        tokio::select! {
            _ = wait => {},
            _ = redraw_receiver.recv() => {},
            Some(event) = event_receiver.recv() => {
                let DomEvent { name, data, element, bubbles } = event;
                let mut rdom = rdom.write().ok()?;
                renderer.handle_event(rdom.get_mut(element)?, name, data, bubbles);
            }
        }

        let mut rdom = rdom.write().ok()?;
        // render after the event has been handled
        let root_id = rdom.root_id();
        renderer.update(rdom.get_mut(root_id)?);

        let mut ctx = SendAnyMap::new();
        ctx.insert(taffy.clone());
        ctx.insert(text_context.clone());

        // update the real dom
        let (to_rerender, _) = rdom.update_state(ctx);

        let size = size.lock().ok()?;

        let width = size.width as f32;
        let height = size.height as f32;
        let size = Size {
            width: AvailableSpace::Definite(width),
            height: AvailableSpace::Definite(height),
        };
        if !to_rerender.is_empty() || last_size != size {
            last_size = size;
            let mut taffy = taffy.lock().unwrap();
            let root_node = rdom.get(rdom.root_id()).unwrap();
            let root_node_layout = root_node.get::<TaffyLayout>().unwrap();
            let root_taffy_node = root_node_layout.node.unwrap();
            let mut style = taffy.style(root_taffy_node).unwrap().clone();
            let new_size = Size {
                width: Dimension::Points(width),
                height: Dimension::Points(height),
            };
            if style.size != new_size {
                style.size = new_size;
                taffy.set_style(root_taffy_node, style).unwrap();
            }
            taffy.compute_layout(root_taffy_node, size).unwrap();
            for k in to_rerender.into_iter() {
                vdom_dirty.insert(k);
            }

            proxy.send_event(Redraw).unwrap();
        }
    }
}

/// A wrapper around the RealDom that manages the lifecycle.
struct DomManager {
    rdom: Arc<RwLock<RealDom>>,
    taffy: Arc<Mutex<Taffy>>,
    size: Arc<Mutex<PhysicalSize<u32>>>,
    /// The node that need to be redrawn.
    dirty: Arc<FxDashSet<NodeId>>,
    force_redraw: bool,
    event_sender: UnboundedSender<DomEvent>,
    redraw_sender: UnboundedSender<()>,
}

impl DomManager {
    fn spawn<R: Driver>(
        rdom: RealDom,
        size: PhysicalSize<u32>,
        spawn_renderer: impl FnOnce(&Arc<RwLock<RealDom>>, &Arc<Mutex<Taffy>>) -> R + Send + 'static,
        proxy: EventLoopProxy<Redraw>,
    ) -> Self {
        let rdom: Arc<RwLock<RealDom>> = Arc::new(RwLock::new(rdom));
        let taffy = Arc::new(Mutex::new(Taffy::new()));
        let size = Arc::new(Mutex::new(size));
        let dirty = Arc::new(FxDashSet::default());

        let (event_sender, event_receiver) = unbounded_channel::<DomEvent>();
        let (redraw_sender, redraw_receiver) = unbounded_channel::<()>();

        let (rdom_clone, size_clone, dirty_clone, taffy_clone) =
            (rdom.clone(), size.clone(), dirty.clone(), taffy.clone());
        // Spawn a thread to run the virtual dom and update the real dom.
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(spawn_dom(
                    rdom_clone,
                    taffy_clone,
                    size_clone,
                    spawn_renderer,
                    proxy,
                    event_receiver,
                    redraw_receiver,
                    dirty_clone,
                ));
        });

        Self {
            rdom,
            taffy,
            size,
            dirty,
            event_sender,
            redraw_sender,
            force_redraw: false,
        }
    }

    fn clean(&self) -> DirtyNodes {
        if self.force_redraw {
            DirtyNodes::All
        } else {
            let dirty = self.dirty.iter().map(|k| *k.key()).collect();
            self.dirty.clear();
            DirtyNodes::Some(dirty)
        }
    }

    fn rdom(&self) -> RwLockWriteGuard<RealDom> {
        self.rdom.write().unwrap()
    }

    fn taffy(&self) -> MutexGuard<Taffy> {
        self.taffy.lock().unwrap()
    }

    fn set_size(&mut self, size: PhysicalSize<u32>) {
        *self.size.lock().unwrap() = size;
        self.force_redraw();
    }

    fn size(&self) -> PhysicalSize<u32> {
        *self.size.lock().unwrap()
    }

    fn force_redraw(&mut self) {
        self.force_redraw = true;
        self.redraw_sender.send(()).unwrap();
    }

    fn render(&self, text_context: &mut TextContext, renderer: &mut SceneBuilder) {
        render(
            &self.rdom(),
            &self.taffy(),
            text_context,
            renderer,
            *self.size.lock().unwrap(),
        );
    }

    fn send_events(&self, events: impl IntoIterator<Item = DomEvent>) {
        for evt in events {
            let _ = self.event_sender.send(evt);
        }
    }
}

pub enum DirtyNodes {
    All,
    Some(FxHashSet<NodeId>),
}

impl DirtyNodes {
    pub fn is_empty(&self) -> bool {
        match self {
            DirtyNodes::All => false,
            DirtyNodes::Some(v) => v.is_empty(),
        }
    }

    #[allow(dead_code)]
    pub fn or(self, other: DirtyNodes) -> DirtyNodes {
        match (self, other) {
            (DirtyNodes::All, _) => DirtyNodes::All,
            (_, DirtyNodes::All) => DirtyNodes::All,
            (DirtyNodes::Some(mut v1), DirtyNodes::Some(v2)) => {
                v1.extend(v2);
                DirtyNodes::Some(v1)
            }
        }
    }
}
