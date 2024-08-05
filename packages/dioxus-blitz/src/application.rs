use crate::waker::{BlitzEvent, BlitzWindowEvent, BlitzWindowId};

use blitz_dom::DocumentLike;
use std::collections::HashMap;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::WindowId;

use crate::{View, WindowConfig};

pub struct Application<Doc: DocumentLike> {
    pub rt: tokio::runtime::Runtime,
    pub windows: HashMap<WindowId, View<Doc>>,
    pub pending_windows: Vec<WindowConfig<Doc>>,
    pub proxy: EventLoopProxy<BlitzEvent<Doc::DocumentEvent>>,

    #[cfg(all(feature = "menu", not(any(target_os = "android", target_os = "ios"))))]
    pub menu_channel: muda::MenuEventReceiver,
}

impl<Doc: DocumentLike> Application<Doc> {
    pub fn new(
        rt: tokio::runtime::Runtime,
        proxy: EventLoopProxy<BlitzEvent<Doc::DocumentEvent>>,
    ) -> Self {
        Application {
            windows: HashMap::new(),
            pending_windows: Vec::new(),
            rt,
            proxy,

            #[cfg(all(feature = "menu", not(any(target_os = "android", target_os = "ios"))))]
            menu_channel: muda::MenuEvent::receiver().clone(),
        }
    }

    pub fn add_window(&mut self, window_config: WindowConfig<Doc>) {
        self.pending_windows.push(window_config);
    }
}

impl<Doc: DocumentLike> ApplicationHandler<BlitzEvent<Doc::DocumentEvent>> for Application<Doc> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Resume existing windows
        for (_, view) in self.windows.iter_mut() {
            view.resume(&self.rt);
        }

        // Initialise pending windows
        for window_config in self.pending_windows.drain(..) {
            let mut view = View::init(window_config, event_loop, &self.proxy);
            view.resume(&self.rt);
            if !view.renderer.is_active() {
                continue;
            }
            self.windows.insert(view.window_id(), view);
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        for (_, view) in self.windows.iter_mut() {
            view.suspend();
        }
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, _cause: winit::event::StartCause) {
        _ = self.proxy.send_event(BlitzEvent::Window {
            window_id: BlitzWindowId::AllWindows,
            data: BlitzWindowEvent::Poll,
        });

        #[cfg(all(feature = "menu", not(any(target_os = "android", target_os = "ios"))))]
        if let Ok(event) = self.menu_channel.try_recv() {
            if event.id == muda::MenuId::new("dev.show_layout") {
                for (_, view) in self.windows.iter_mut() {
                    view.devtools.show_layout = !view.devtools.show_layout;
                    view.request_redraw();
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        // Exit the app when window close is requested. TODO: Only exit when last window is closed.
        if matches!(event, WindowEvent::CloseRequested) {
            event_loop.exit();
            return;
        }

        if let Some(window) = self.windows.get_mut(&window_id) {
            window.handle_winit_event(event);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: BlitzEvent<Doc::DocumentEvent>) {
        match event {
            BlitzEvent::Window { data, window_id } => match window_id {
                BlitzWindowId::AllWindows => {
                    for view in self.windows.values_mut() {
                        view.handle_blitz_event(data.clone());
                    }
                }
                BlitzWindowId::SpecificWindow(window_id) => {
                    if let Some(view) = self.windows.get_mut(&window_id) {
                        view.handle_blitz_event(data);
                    };
                }
            },
            BlitzEvent::Exit => event_loop.exit(),
        }
    }
}
