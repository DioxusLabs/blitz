use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex, MutexGuard},
};

use anymap::AnyMap;
use dioxus::prelude::{Component, VirtualDom};

use piet_wgpu::{Piet, WgpuRenderer};
use tao::{dpi::PhysicalSize, event_loop::EventLoopProxy, window::Window};

use crate::{render::render, Dom, Redraw};
use dioxus::native_core::real_dom::RealDom;
use taffy::{prelude::Number, Taffy};

pub struct ApplicationState {
    dom: DomManager,
    wgpu_renderer: WgpuRenderer,
}

impl ApplicationState {
    /// Create a new window state and spawn a vdom thread.
    pub fn new(root: Component<()>, window: &Window, proxy: EventLoopProxy<Redraw>) -> Self {
        let inner_size = window.inner_size();

        let dom = DomManager::spawn(inner_size, root, proxy);

        let mut wgpu_renderer = WgpuRenderer::new(window).unwrap();
        wgpu_renderer.set_size(piet_wgpu::kurbo::Size {
            width: inner_size.width as f64,
            height: inner_size.height as f64,
        });
        wgpu_renderer.set_scale(1.0);

        ApplicationState { dom, wgpu_renderer }
    }

    pub fn render(&mut self) {
        let mut r = Piet::new(&mut self.wgpu_renderer);
        self.dom.render(&mut r);
    }

    pub fn set_size(&mut self, size: PhysicalSize<u32>) {
        self.dom.set_size(size);
        self.wgpu_renderer.set_size(piet_wgpu::kurbo::Size {
            width: size.width as f64,
            height: size.height as f64,
        });
    }

    pub fn clean(&self) -> Vec<usize> {
        self.dom.clean()
    }
}

/// A wrapper around the DOM that manages the lifecycle of the VDom and RealDom.
struct DomManager {
    rdom: Arc<Mutex<Dom>>,
    size: Arc<Mutex<PhysicalSize<u32>>>,
    /// The node that need to be redrawn.
    dirty: Arc<Mutex<Vec<usize>>>,
}

impl DomManager {
    fn spawn(size: PhysicalSize<u32>, root: Component<()>, proxy: EventLoopProxy<Redraw>) -> Self {
        let rdom: Arc<Mutex<Dom>> = Arc::new(Mutex::new(RealDom::new()));
        let size = Arc::new(Mutex::new(size));
        let dirty = Arc::new(Mutex::new(Vec::new()));

        let weak_rdom = Arc::downgrade(&rdom);
        let weak_size = Arc::downgrade(&size);
        let weak_dirty = Arc::downgrade(&dirty);

        // Spawn a thread to run the virtual dom and update the real dom.
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let stretch = Rc::new(RefCell::new(Taffy::new()));
                    let mut vdom = VirtualDom::new(root);
                    let mutations = vdom.rebuild();
                    let mut last_size = taffy::prelude::Size::undefined();
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

                                let size = taffy::prelude::Size {
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
                        vdom.wait_for_work().await;

                        if let Some(strong) = weak_rdom.upgrade() {
                            if let Ok(mut rdom) = strong.lock() {
                                let mutations = vdom.work_with_deadline(|| false);
                                // update the real dom's nodes

                                let to_update = rdom.apply_mutations(mutations);

                                let mut ctx = AnyMap::new();
                                ctx.insert(stretch.clone());

                                // update the style and layout
                                let to_rerender = rdom.update_state(&vdom, to_update, ctx).unwrap();

                                if let Some(strong) = weak_size.upgrade() {
                                    let size = strong.lock().unwrap();

                                    let size = taffy::prelude::Size {
                                        width: Number::Defined(size.width as f32),
                                        height: Number::Defined(size.height as f32),
                                    };
                                    if !to_rerender.is_empty() || last_size != size {
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
        Self { rdom, size, dirty }
    }

    fn clean(&self) -> Vec<usize> {
        std::mem::take(&mut *self.dirty.lock().unwrap())
    }

    fn rdom(&self) -> MutexGuard<Dom> {
        let r = self.rdom.lock().unwrap();

        r
    }

    fn set_size(&self, size: PhysicalSize<u32>) {
        *self.size.lock().unwrap() = size;
    }

    fn render(&self, renderer: &mut Piet) {
        render(&self.rdom(), renderer, *self.size.lock().unwrap());
    }
}
