use dioxus::prelude::{Component, VirtualDom};
use std::{
    any::Any,
    sync::{Arc, Mutex, MutexGuard, Weak},
};
use tao::{dpi::PhysicalSize, event_loop::EventLoopProxy, window::Window};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use vello::{
    util::{RenderContext, RenderSurface},
    Renderer, Scene, SceneBuilder,
};

use crate::{
    events::{AnyEvent, BlitzEventHandler},
    focus::FocusState,
    render::render,
    text::TextContext,
    Dom, Redraw, TaoEvent,
};
use dioxus_native_core::{real_dom::RealDom, tree::TreeView, FxDashSet, NodeId, SendAnyMap};
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
    wgpu_renderer: Renderer,
    event_handler: Arc<Mutex<BlitzEventHandler>>,
}

impl ApplicationState {
    /// Create a new window state and spawn a vdom thread.
    pub async fn new(root: Component<()>, window: &Window, proxy: EventLoopProxy<Redraw>) -> Self {
        let inner_size = window.inner_size();

        let focus_state = Arc::new(Mutex::new(FocusState::default()));
        let weak_focus_state = Arc::downgrade(&focus_state);

        let event_handler = Arc::new(Mutex::new(BlitzEventHandler::new(focus_state)));
        let weak_event_handler = Arc::downgrade(&event_handler);

        let dom = DomManager::spawn(
            inner_size,
            root,
            proxy,
            weak_event_handler,
            weak_focus_state,
        );

        let render_context = RenderContext::new().await.unwrap();
        let size = window.inner_size();
        let surface = render_context.create_surface(window, size.width, size.height);
        let wgpu_renderer = Renderer::new(&render_context.device).unwrap();

        let text_context = TextContext::default();

        ApplicationState {
            dom,
            text_context,
            render_context,
            wgpu_renderer,
            surface,
            event_handler,
        }
    }

    pub fn render(&mut self) {
        let mut scene = Scene::new();
        let mut builder = SceneBuilder::for_scene(&mut scene);
        self.dom.render(&mut self.text_context, &mut builder);
        builder.finish();
        let surface_texture = self
            .surface
            .surface
            .get_current_texture()
            .expect("failed to get surface texture");
        self.wgpu_renderer
            .render_to_surface(
                &self.render_context.device,
                &self.render_context.queue,
                &scene,
                &surface_texture,
                self.surface.config.width,
                self.surface.config.height,
            )
            .expect("failed to render to surface");
        surface_texture.present();
        self.render_context.device.poll(wgpu::Maintain::Wait);
    }

    pub fn set_size(&mut self, size: PhysicalSize<u32>) {
        // the window size is zero when minimized which causes the renderer to panic
        if size.width > 0 && size.height > 0 {
            self.dom.set_size(size);
            self.render_context
                .resize_surface(&mut self.surface, size.width, size.height);
        }
    }

    pub fn clean(&self) -> DirtyNodes {
        let dirty = self.dom.clean();
        if self.event_handler.lock().unwrap().clean() {
            DirtyNodes::All
        } else {
            dirty
        }
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
            let mut event_handler = self.event_handler.lock().unwrap();
            event_handler.register_event(event, rdom, &size);
            evts = event_handler.drain_events();
        }
        self.dom.send_events(evts);
    }
}

#[allow(clippy::too_many_arguments)]
async fn spawn_dom(
    rdom: Arc<Mutex<Dom>>,
    size: Arc<Mutex<PhysicalSize<u32>>>,
    root: Component<()>,
    proxy: EventLoopProxy<Redraw>,
    event_handler: Weak<Mutex<BlitzEventHandler>>,
    focus_state: Weak<Mutex<FocusState>>,
    mut event_receiver: UnboundedReceiver<AnyEvent>,
    mut redraw_receiver: UnboundedReceiver<()>,
    vdom_dirty: Arc<FxDashSet<NodeId>>,
) -> Option<()> {
    let taffy = Arc::new(Mutex::new(Taffy::new()));
    let mut vdom = VirtualDom::new(root);
    let mutations = vdom.rebuild();
    let mut last_size;

    {
        let mut rdom = rdom.lock().ok()?;
        // update the real dom's nodes
        let (to_update, _) = rdom.apply_mutations(mutations);
        let mut ctx = SendAnyMap::new();
        ctx.insert(taffy.clone());
        // update the style and layout
        let to_rerender = rdom.update_state(to_update, ctx);
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
        let root_node = rdom[NodeId(0)].state.layout.node.unwrap();

        let mut style = *locked_taffy.style(root_node).unwrap();
        style.size = Size {
            width: Dimension::Points(width),
            height: Dimension::Points(height),
        };
        locked_taffy.set_style(root_node, style).unwrap();
        locked_taffy
            .compute_layout(rdom[NodeId(0)].state.layout.node.unwrap(), size)
            .unwrap();
        rdom.traverse_depth_first_mut(|n| {
            if let Some(node) = n.state.layout.node {
                n.state.layout.layout = Some(*locked_taffy.layout(node).unwrap());
            }
        });
        for k in to_rerender.into_iter() {
            vdom_dirty.insert(k);
        }
        proxy.send_event(Redraw).unwrap();
    }

    loop {
        let wait = vdom.wait_for_work();
        tokio::select! {
            _ = wait=>{},
            _ = redraw_receiver.recv()=>{},
            Some(event) = event_receiver.recv()=>{
                let name = event.name;
                let any_value:Box<dyn Any> = event.data;
                let data = any_value.into();
                let element = event.element;
                let bubbles = event.bubbles;
                vdom.handle_event(name, data, element, bubbles);
            }
        }

        let mut rdom = rdom.lock().ok()?;
        let mutations = vdom.render_immediate();
        let strong = focus_state.upgrade()?;
        let mut focus_state = strong.lock().ok()?;
        let strong = event_handler.upgrade()?;
        let mut event_handler = strong.lock().ok()?;
        event_handler.prune(&mutations, &rdom);
        focus_state.prune(&mutations, &rdom);

        // update the real dom's nodes
        let (to_update, _) = rdom.apply_mutations(mutations);

        let mut ctx = SendAnyMap::new();
        ctx.insert(taffy.clone());

        // update the style and layout
        let to_rerender = rdom.update_state(to_update, ctx);

        let size = size.lock().ok()?;

        let width = size.width as f32;
        let height = size.height as f32;
        let size = Size {
            width: AvailableSpace::Definite(width),
            height: AvailableSpace::Definite(height),
        };
        if !to_rerender.is_empty() || last_size != size {
            last_size = size;
            let mut locked_taffy = taffy.lock().unwrap();
            let root_node = rdom[NodeId(0)].state.layout.node.unwrap();
            let mut style = *locked_taffy.style(root_node).unwrap();
            style.size = Size {
                width: Dimension::Points(width),
                height: Dimension::Points(height),
            };
            locked_taffy.set_style(root_node, style).unwrap();
            locked_taffy
                .compute_layout(rdom[NodeId(0)].state.layout.node.unwrap(), size)
                .unwrap();
            rdom.traverse_depth_first_mut(|n| {
                if let Some(node) = n.state.layout.node {
                    n.state.layout.layout = Some(*locked_taffy.layout(node).unwrap());
                }
            });
            for k in to_rerender.into_iter() {
                vdom_dirty.insert(k);
            }

            proxy.send_event(Redraw).unwrap();
        }
    }
}

/// A wrapper around the DOM that manages the lifecycle of the VDom and RealDom.
struct DomManager {
    rdom: Arc<Mutex<Dom>>,
    size: Arc<Mutex<PhysicalSize<u32>>>,
    /// The node that need to be redrawn.
    dirty: Arc<FxDashSet<NodeId>>,
    force_redraw: bool,
    event_sender: UnboundedSender<AnyEvent>,
    redraw_sender: UnboundedSender<()>,
}

impl DomManager {
    fn spawn(
        size: PhysicalSize<u32>,
        root: Component<()>,
        proxy: EventLoopProxy<Redraw>,
        event_handler: Weak<Mutex<BlitzEventHandler>>,
        focus_state: Weak<Mutex<FocusState>>,
    ) -> Self {
        let rdom: Arc<Mutex<Dom>> = Arc::new(Mutex::new(RealDom::new()));
        let size = Arc::new(Mutex::new(size));
        let dirty = Arc::new(FxDashSet::default());

        let (event_sender, event_receiver) = unbounded_channel::<AnyEvent>();
        let (redraw_sender, redraw_receiver) = unbounded_channel::<()>();

        let (rdom_clone, size_clone, dirty_clone) = (rdom.clone(), size.clone(), dirty.clone());
        // Spawn a thread to run the virtual dom and update the real dom.
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(spawn_dom(
                    rdom_clone,
                    size_clone,
                    root,
                    proxy,
                    event_handler,
                    focus_state,
                    event_receiver,
                    redraw_receiver,
                    dirty_clone,
                ));
        });

        Self {
            rdom,
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
            let dirty: Vec<NodeId> = self.dirty.iter().map(|k| *k.key()).collect();
            self.dirty.clear();
            DirtyNodes::Some(dirty)
        }
    }

    fn rdom(&self) -> MutexGuard<Dom> {
        self.rdom.lock().unwrap()
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
            text_context,
            renderer,
            *self.size.lock().unwrap(),
        );
    }

    fn send_events(&self, events: impl IntoIterator<Item = AnyEvent>) {
        for evt in events {
            let _ = self.event_sender.send(evt);
        }
    }
}

pub enum DirtyNodes {
    All,
    Some(Vec<NodeId>),
}

impl DirtyNodes {
    pub fn is_empty(&self) -> bool {
        match self {
            DirtyNodes::All => false,
            DirtyNodes::Some(v) => v.is_empty(),
        }
    }
}
