use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex, MutexGuard, Weak},
};

use anymap::AnyMap;
use dioxus::{
    core::{exports::futures_channel::mpsc::unbounded, SchedulerMsg, UserEvent},
    native_core::utils::PersistantElementIter,
    prelude::{Component, UnboundedSender, VirtualDom},
};

use futures_util::StreamExt;
use piet_wgpu::{Piet, WgpuRenderer};
use tao::{dpi::PhysicalSize, event_loop::EventLoopProxy, window::Window};

use crate::{events::BlitzEventHandler, render::render, Dom, Redraw, TaoEvent};
use dioxus::native_core::real_dom::RealDom;
use taffy::{
    prelude::{Number, Size},
    Taffy,
};

pub struct ApplicationState {
    dom: DomManager,
    wgpu_renderer: WgpuRenderer,
    event_handler: BlitzEventHandler,
}

impl ApplicationState {
    /// Create a new window state and spawn a vdom thread.
    pub fn new(root: Component<()>, window: &Window, proxy: EventLoopProxy<Redraw>) -> Self {
        let inner_size = window.inner_size();

        let focus_iter = Arc::new(Mutex::new(PersistantElementIter::default()));
        let weak_focus_iter = Arc::downgrade(&focus_iter);

        let event_handler = BlitzEventHandler::new(focus_iter);

        let dom = DomManager::spawn(inner_size, root, proxy, weak_focus_iter);

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
        self.dom.clean()
    }

    pub fn send_event(&mut self, event: &TaoEvent) {
        if self
            .event_handler
            .register_event(event, &mut self.dom.rdom())
        {
            self.force_redraw()
        }
        let evts = self.event_handler.drain_events();
        self.dom.send_events(evts);
    }

    pub fn force_redraw(&mut self) {
        self.dom.force_redraw();
    }
}

/// A wrapper around the DOM that manages the lifecycle of the VDom and RealDom.
struct DomManager {
    rdom: Arc<Mutex<Dom>>,
    size: Arc<Mutex<PhysicalSize<u32>>>,
    /// The node that need to be redrawn.
    dirty: Arc<Mutex<Vec<usize>>>,
    force_redraw: bool,
    scheduler: UnboundedSender<SchedulerMsg>,
    redraw_sender: UnboundedSender<()>,
}

impl DomManager {
    fn spawn(
        size: PhysicalSize<u32>,
        root: Component<()>,
        proxy: EventLoopProxy<Redraw>,
        weak_focus_iter: Weak<Mutex<PersistantElementIter>>,
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
                            let to_rerender = rdom.update_state(&vdom, to_update, ctx).unwrap();
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
                                        rdom[rdom.root_id()].state.layout.node.unwrap(),
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
                                if let Some(strong) = weak_focus_iter.upgrade() {
                                    if let Ok(mut focus_iter) = strong.lock() {
                                        let mutations = vdom.work_with_deadline(|| false);

                                        for m in &mutations {
                                            focus_iter.prune(m, &rdom);
                                        }

                                        // update the real dom's nodes
                                        let to_update = rdom.apply_mutations(mutations);

                                        let mut ctx = AnyMap::new();
                                        ctx.insert(stretch.clone());

                                        // update the style and layout
                                        let to_rerender =
                                            rdom.update_state(&vdom, to_update, ctx).unwrap();

                                        if let Some(strong) = weak_size.upgrade() {
                                            let size = strong.lock().unwrap();

                                            let size = Size {
                                                width: Number::Defined(size.width as f32),
                                                height: Number::Defined(size.height as f32),
                                            };
                                            if !to_rerender.is_empty() || last_size != size {
                                                last_size = size.clone();
                                                stretch
                                                    .borrow_mut()
                                                    .compute_layout(
                                                        rdom[rdom.root_id()]
                                                            .state
                                                            .layout
                                                            .node
                                                            .unwrap(),
                                                        size,
                                                    )
                                                    .unwrap();
                                                rdom.traverse_depth_first_mut(|n| {
                                                    if let Some(node) = n.state.layout.node {
                                                        n.state.layout.layout = Some(
                                                            *stretch.borrow().layout(node).unwrap(),
                                                        );
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
            DirtyNodes::Some(std::mem::replace(
                &mut *self.dirty.lock().unwrap(),
                Vec::new(),
            ))
        }
    }

    fn rdom(&self) -> MutexGuard<Dom> {
        self.rdom.lock().unwrap()
    }

    fn set_size(&mut self, size: PhysicalSize<u32>) {
        *self.size.lock().unwrap() = size;
        self.force_redraw();
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
    Some(Vec<usize>),
}

impl DirtyNodes {
    pub fn is_empty(&self) -> bool {
        match self {
            DirtyNodes::All => false,
            DirtyNodes::Some(v) => v.is_empty(),
        }
    }
}
