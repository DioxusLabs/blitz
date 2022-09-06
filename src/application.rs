use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex, MutexGuard, Weak},
};

use anymap::AnyMap;
use dioxus::core::ElementId;
use dioxus::{
    core::{exports::futures_channel::mpsc::unbounded, SchedulerMsg, UserEvent},
    prelude::{Component, UnboundedSender, VirtualDom},
};

use futures_util::StreamExt;
use piet_wgpu::{Piet, WgpuRenderer};
use tao::{dpi::PhysicalSize, event_loop::EventLoopProxy, window::Window};

use crate::{events::BlitzEventHandler, focus::FocusState, render::render, Dom, Redraw, TaoEvent};
use dioxus_native_core::real_dom::RealDom;
use taffy::{
    prelude::{Number, Size},
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
    dirty: Arc<Mutex<Vec<ElementId>>>,
    force_redraw: bool,
    scheduler: UnboundedSender<SchedulerMsg>,
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
        let dirty = Arc::new(Mutex::new(Vec::new()));

        let weak_rdom = Arc::downgrade(&rdom);
        let weak_size = Arc::downgrade(&size);
        let weak_dirty = Arc::downgrade(&dirty);

        let channel_sender = Arc::new(Mutex::new(None));
        let channel_sender_weak = Arc::downgrade(&channel_sender);

        let (redraw_sender, mut redraw_receiver) = unbounded::<()>();

        // Spawn a thread to run the virtual dom and update the real dom.
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let stretch = Rc::new(RefCell::new(Taffy::new()));
                    let mut vdom = VirtualDom::new(root);
                    channel_sender_weak
                        .upgrade()
                        .unwrap()
                        .lock()
                        .unwrap()
                        .replace(vdom.get_scheduler_channel());
                    let mutations = vdom.rebuild();
                    let mut last_size = Size::undefined();
                    if let Some(strong) = weak_rdom.upgrade() {
                        if let Ok(mut rdom) = strong.lock() {
                            // update the real dom's nodes
                            let to_update = rdom.apply_mutations(vec![mutations]);
                            let mut ctx = AnyMap::new();
                            ctx.insert(stretch.clone());
                            // update the style and layout
                            let to_rerender = rdom.update_state(&vdom, to_update, ctx);
                            if let Some(strong) = weak_size.upgrade() {
                                let size = strong.lock().unwrap();

                                let size = Size {
                                    width: Number::Defined(size.width as f32),
                                    height: Number::Defined(size.height as f32),
                                };

                                last_size = size;

                                stretch
                                    .borrow_mut()
                                    .compute_layout(
                                        rdom[ElementId(rdom.root_id())].state.layout.node.unwrap(),
                                        size,
                                    )
                                    .unwrap();
                                rdom.traverse_depth_first_mut(|n| {
                                    if let Some(node) = n.state.layout.node {
                                        n.state.layout.layout =
                                            Some(*stretch.borrow().layout(node).unwrap());
                                    }
                                });
                                weak_dirty
                                    .upgrade()
                                    .unwrap()
                                    .lock()
                                    .unwrap()
                                    .extend(to_rerender.iter());
                                proxy.send_event(Redraw).unwrap();
                            }
                        }
                    }
                    loop {
                        let wait = vdom.wait_for_work();
                        tokio::select! {
                            _ = wait=>{},
                            _ = redraw_receiver.next()=>{},
                        }

                        if let Some(strong) = weak_rdom.upgrade() {
                            if let Ok(mut rdom) = strong.lock() {
                                let mutations = vdom.work_with_deadline(|| false);
                                if let Some(strong) = weak_focus_state.upgrade() {
                                    if let Ok(mut focus_state) = strong.lock() {
                                        if let Some(strong) = weak_event_handler.upgrade() {
                                            if let Ok(mut event_handler) = strong.lock() {
                                                for m in &mutations {
                                                    event_handler.prune(m, &rdom);
                                                    focus_state.prune(m, &rdom);
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
                                } else {
                                    break;
                                }
                                // update the real dom's nodes
                                let to_update = rdom.apply_mutations(mutations);

                                let mut ctx = AnyMap::new();
                                ctx.insert(stretch.clone());

                                // update the style and layout
                                let to_rerender = rdom.update_state(&vdom, to_update, ctx);

                                if let Some(strong) = weak_size.upgrade() {
                                    let size = *strong.lock().unwrap();

                                    let size = Size {
                                        width: Number::Defined(size.width as f32),
                                        height: Number::Defined(size.height as f32),
                                    };
                                    if !to_rerender.is_empty() || last_size != size {
                                        last_size = size;
                                        stretch
                                            .borrow_mut()
                                            .compute_layout(
                                                rdom[ElementId(rdom.root_id())]
                                                    .state
                                                    .layout
                                                    .node
                                                    .unwrap(),
                                                size,
                                            )
                                            .unwrap();
                                        rdom.traverse_depth_first_mut(|n| {
                                            if let Some(node) = n.state.layout.node {
                                                n.state.layout.layout =
                                                    Some(*stretch.borrow().layout(node).unwrap());
                                            }
                                        });
                                        weak_dirty
                                            .upgrade()
                                            .unwrap()
                                            .lock()
                                            .unwrap()
                                            .extend(to_rerender.into_iter());

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

        while channel_sender.lock().unwrap().is_none() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let mut sender_lock = channel_sender.lock().unwrap();
        Self {
            rdom,
            size,
            dirty,
            scheduler: sender_lock.take().unwrap(),
            redraw_sender,
            force_redraw: false,
        }
    }

    fn clean(&self) -> DirtyNodes {
        if self.force_redraw {
            DirtyNodes::All
        } else {
            DirtyNodes::Some(std::mem::take(&mut *self.dirty.lock().unwrap()))
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
        self.redraw_sender.unbounded_send(()).unwrap();
    }

    fn render(&self, renderer: &mut Piet) {
        render(&self.rdom(), renderer, *self.size.lock().unwrap());
    }

    fn send_events(&self, events: Vec<UserEvent>) {
        for evt in events {
            self.scheduler
                .unbounded_send(SchedulerMsg::Event(evt))
                .unwrap();
        }
    }
}

pub enum DirtyNodes {
    All,
    Some(Vec<ElementId>),
}

impl DirtyNodes {
    pub fn is_empty(&self) -> bool {
        match self {
            DirtyNodes::All => false,
            DirtyNodes::Some(v) => v.is_empty(),
        }
    }
}
