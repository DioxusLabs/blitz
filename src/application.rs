use dioxus::prelude::{Component, VirtualDom};
use piet_wgpu::{Piet, WgpuRenderer};
use std::{
    any::Any,
    sync::{Arc, Mutex, MutexGuard, Weak},
};
use tao::{dpi::PhysicalSize, event_loop::EventLoopProxy, window::Window};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

use crate::{
    events::{AnyEvent, BlitzEventHandler},
    focus::FocusState,
    render::render,
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
    wgpu_renderer: WgpuRenderer,
    event_handler: Arc<Mutex<BlitzEventHandler>>,
}

impl ApplicationState {
    /// Create a new window state and spawn a vdom thread.
    pub fn new(root: Component<()>, window: &Window, proxy: EventLoopProxy<Redraw>) -> Self {
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

        let mut wgpu_renderer = WgpuRenderer::new(window).unwrap();
        wgpu_renderer.set_size(piet_wgpu::kurbo::Size {
            width: inner_size.width as f64,
            height: inner_size.height as f64,
        });
        wgpu_renderer.set_scale(1.0);

        ApplicationState {
            dom,
            wgpu_renderer,
            event_handler,
        }
    }

    pub fn render(&mut self) {
        let mut r = Piet::new(&mut self.wgpu_renderer);
        self.dom.render(&mut r);
    }

    pub fn set_size(&mut self, size: PhysicalSize<u32>) {
        // the window size is zero when minimized which causes the renderer to panic
        if size.width > 0 && size.height > 0 {
            self.dom.set_size(size);
            self.wgpu_renderer.set_size(piet_wgpu::kurbo::Size {
                width: size.width as f64,
                height: size.height as f64,
            });
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

/// A wrapper around the DOM that manages the lifecycle of the VDom and RealDom.
struct DomManager {
    rdom: Arc<Mutex<Dom>>,
    size: Arc<Mutex<PhysicalSize<u32>>>,
    /// The node that need to be redrawn.
    dirty: FxDashSet<NodeId>,
    force_redraw: bool,
    event_sender: UnboundedSender<AnyEvent>,
    redraw_sender: UnboundedSender<()>,
}

impl DomManager {
    fn spawn(
        size: PhysicalSize<u32>,
        root: Component<()>,
        proxy: EventLoopProxy<Redraw>,
        weak_event_handler: Weak<Mutex<BlitzEventHandler>>,
        weak_focus_state: Weak<Mutex<FocusState>>,
    ) -> Self {
        let rdom: Arc<Mutex<Dom>> = Arc::new(Mutex::new(RealDom::new()));
        let size = Arc::new(Mutex::new(size));
        let dirty = FxDashSet::default();

        let weak_rdom = Arc::downgrade(&rdom);
        let weak_size = Arc::downgrade(&size);
        let vdom_dirty = dirty.clone();

        let (event_sender, mut event_receiver) = unbounded_channel::<AnyEvent>();
        let (redraw_sender, mut redraw_receiver) = unbounded_channel::<()>();

        // Spawn a thread to run the virtual dom and update the real dom.
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let taffy = Arc::new(Mutex::new(Taffy::new()));
                    let mut vdom = VirtualDom::new(root);
                    let mutations = vdom.rebuild();
                    let mut last_size = Size::MAX_CONTENT;
                    if let Some(strong) = weak_rdom.upgrade() {
                        if let Ok(mut rdom) = strong.lock() {
                            // update the real dom's nodes
                            let (to_update, _) = rdom.apply_mutations(mutations);
                            let mut ctx = SendAnyMap::new();
                            ctx.insert(taffy.clone());
                            // update the style and layout
                            let to_rerender = rdom.update_state(to_update, ctx);
                            if let Some(strong) = weak_size.upgrade() {
                                let size = strong.lock().unwrap();

                                let width = size.width as f32;
                                let height = size.height as f32;
                                let size = Size {
                                    width: AvailableSpace::Definite(width),
                                    height: AvailableSpace::Definite(height),
                                };

                                last_size = size;

                                let mut locked_taffy = taffy.lock().unwrap();

                                let root_node = rdom[NodeId(0)].state.layout.node.unwrap();

                                // the root node fills the entire area

                                let mut style = *locked_taffy.style(root_node).unwrap();
                                style.size = Size {
                                    width: Dimension::Points(width),
                                    height: Dimension::Points(height),
                                };
                                locked_taffy.set_style(root_node, style).unwrap();
                                locked_taffy
                                    .compute_layout(
                                        rdom[NodeId(0)].state.layout.node.unwrap(),
                                        size,
                                    )
                                    .unwrap();
                                rdom.traverse_depth_first_mut(|n| {
                                    if let Some(node) = n.state.layout.node {
                                        n.state.layout.layout =
                                            Some(*locked_taffy.layout(node).unwrap());
                                    }
                                });
                                for k in to_rerender.into_iter() {
                                    vdom_dirty.insert(k);
                                }
                                proxy.send_event(Redraw).unwrap();
                            }
                        }
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

                        if let Some(strong) = weak_rdom.upgrade() {
                            if let Ok(mut rdom) = strong.lock() {
                                let mutations = vdom.render_immediate();
                                if let Some(strong) = weak_focus_state.upgrade() {
                                    if let Ok(mut focus_state) = strong.lock() {
                                        if let Some(strong) = weak_event_handler.upgrade() {
                                            if let Ok(mut event_handler) = strong.lock() {
                                                event_handler.prune(&mutations, &rdom);
                                                focus_state.prune(&mutations, &rdom);
                                            } else {
                                                break;
                                            }
                                        } else {
                                            break;
                                        }
                                    } else {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                                // update the real dom's nodes
                                let (to_update, _) = rdom.apply_mutations(mutations);

                                let mut ctx = SendAnyMap::new();
                                ctx.insert(taffy.clone());

                                // update the style and layout
                                let to_rerender = rdom.update_state(to_update, ctx);

                                if let Some(strong) = weak_size.upgrade() {
                                    let size = *strong.lock().unwrap();

                                    let size = Size {
                                        width: AvailableSpace::Definite(size.width as f32),
                                        height: AvailableSpace::Definite(size.height as f32),
                                    };
                                    if !to_rerender.is_empty() || last_size != size {
                                        last_size = size;
                                        let mut locked_taffy = taffy.lock().unwrap();
                                        locked_taffy
                                            .compute_layout(
                                                rdom[NodeId(0)].state.layout.node.unwrap(),
                                                size,
                                            )
                                            .unwrap();
                                        rdom.traverse_depth_first_mut(|n| {
                                            if let Some(node) = n.state.layout.node {
                                                n.state.layout.layout =
                                                    Some(*locked_taffy.layout(node).unwrap());
                                            }
                                        });
                                        for k in to_rerender.into_iter() {
                                            vdom_dirty.insert(k);
                                        }

                                        proxy.send_event(Redraw).unwrap();
                                    }
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                });
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

    fn render(&self, renderer: &mut Piet) {
        render(&self.rdom(), renderer, *self.size.lock().unwrap());
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
