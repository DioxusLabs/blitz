use std::{
    any::Any,
    sync::{Arc, Mutex},
    time::Duration,
};

use dioxus::prelude::{Component, VirtualDom};
use druid_shell::{
    kurbo::Size,
    piet::{Color, Piet, RenderContext},
    Application, Cursor, KeyEvent, MouseEvent, Region, TimerToken, WinHandler, WindowHandle,
};

use crate::{render::render, Dom};
use dioxus::native_core::real_dom::RealDom;
use stretch2::{prelude::Number, Stretch};

const BG_COLOR: Color = Color::BLACK;

pub struct WinState {
    size: Arc<Mutex<Size>>,
    handle: WindowHandle,
    real_dom: Arc<Mutex<Dom>>,
    dirty: Arc<Mutex<bool>>,
}

impl WinHandler for WinState {
    fn connect(&mut self, handle: &WindowHandle) {
        self.handle = handle.clone();
        self.handle.request_timer(Duration::from_millis(10));
    }

    fn prepare_paint(&mut self) {
        self.handle.invalidate();
    }

    fn paint(&mut self, piet: &mut Piet, _: &Region) {
        let rect = self.size.lock().unwrap().clone().to_rect();

        piet.fill(rect, &BG_COLOR);
        render(&self.real_dom.lock().unwrap(), piet);
    }

    fn size(&mut self, size: Size) {
        *self.size.lock().unwrap() = size;
    }

    fn request_close(&mut self) {
        self.handle.close();
    }

    fn destroy(&mut self) {
        Application::global().quit()
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    fn key_down(&mut self, _event: KeyEvent) -> bool {
        // println!("keydown: {:?}", event);
        false
    }

    fn key_up(&mut self, _event: KeyEvent) {
        // println!("keyup: {:?}", event);
    }

    fn wheel(&mut self, _event: &MouseEvent) {
        // println!("mouse_wheel {:?}", event);
    }

    fn mouse_move(&mut self, _event: &MouseEvent) {
        self.handle.set_cursor(&Cursor::Arrow);
        // println!("mouse_move {:?}", event);
    }

    fn mouse_down(&mut self, _event: &MouseEvent) {
        // println!("mouse_down {:?}", event);
    }

    fn mouse_up(&mut self, _event: &MouseEvent) {
        // vdom.handle_message(SchedulerMsg::Event(e));
        // println!("mouse_up {:?}", event);
    }

    // druid_shell has no update loop, so we have to continuously create timers to check for updates
    fn timer(&mut self, _token: TimerToken) {
        if *self.dirty.lock().unwrap() {
            self.handle.invalidate();
            *self.dirty.lock().unwrap() = false;
        }
        self.handle.request_timer(Duration::from_millis(10));
    }
}

impl WinState {
    /// Create a new window state and spawn a vdom thread.
    pub fn new(root: Component<()>) -> Self {
        let rdom: Arc<Mutex<Dom>> = Arc::new(Mutex::new(RealDom::new()));
        let size = Arc::new(Mutex::new(Size::default()));
        let dirty = Arc::new(Mutex::new(true));

        // Spawn a thread to run the virtual dom and update the real dom.
        let weak_rdom = Arc::downgrade(&rdom);
        let weak_size = Arc::downgrade(&size);
        let weak_dirty = Arc::downgrade(&dirty);

        std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let mut stretch = Stretch::new();
                    let mut vdom = VirtualDom::new(root);
                    let mutations = vdom.rebuild();
                    let mut last_size = stretch2::prelude::Size::undefined();
                    if let Some(strong) = weak_rdom.upgrade() {
                        if let Ok(mut rdom) = strong.lock() {
                            // update the real dom's nodes
                            let to_update = rdom.apply_mutations(vec![mutations]);
                            // update the style and layout
                            let _to_rerender = rdom
                                .update_state(&vdom, to_update, &mut stretch, &mut ())
                                .unwrap();
                            if let Some(strong) = weak_size.upgrade() {
                                let size = strong.lock().unwrap().clone();
                                let size = stretch2::prelude::Size {
                                    width: Number::Defined(size.width as f32),
                                    height: Number::Defined(size.height as f32),
                                };
                                last_size = size;
                                stretch
                                    .compute_layout(
                                        rdom[rdom.root_id()].up_state.node.unwrap(),
                                        size,
                                    )
                                    .unwrap();
                                rdom.traverse_depth_first_mut(|n| {
                                    if let Some(node) = n.up_state.node {
                                        n.up_state.layout = Some(*stretch.layout(node).unwrap());
                                    }
                                });
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
                                // update the style and layout
                                let to_rerender = rdom
                                    .update_state(&vdom, to_update, &mut stretch, &mut ())
                                    .unwrap();
                                if let Some(strong) = weak_size.upgrade() {
                                    let size = strong.lock().unwrap().clone();
                                    let size = stretch2::prelude::Size {
                                        width: Number::Defined(size.width as f32),
                                        height: Number::Defined(size.height as f32),
                                    };
                                    if !to_rerender.is_empty() || last_size != size {
                                        last_size = size.clone();
                                        stretch
                                            .compute_layout(
                                                rdom[rdom.root_id()].up_state.node.unwrap(),
                                                size,
                                            )
                                            .unwrap();
                                        rdom.traverse_depth_first_mut(|n| {
                                            if let Some(node) = n.up_state.node {
                                                n.up_state.layout =
                                                    Some(*stretch.layout(node).unwrap());
                                            }
                                        });
                                        *weak_dirty.upgrade().unwrap().lock().unwrap() = true;
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

        WinState {
            size,
            handle: WindowHandle::default(),
            real_dom: rdom,
            dirty,
        }
    }
}
