use crate::event::BlitzShellEvent;

use anyrender::WindowRenderer;
use std::collections::HashMap;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::WindowId;

use crate::{View, WindowConfig};

pub struct BlitzApplication<Rend: WindowRenderer> {
    pub windows: HashMap<WindowId, View<Rend>>,
    pub pending_windows: Vec<WindowConfig<Rend>>,
    pub proxy: EventLoopProxy<BlitzShellEvent>,
}

impl<Rend: WindowRenderer> BlitzApplication<Rend> {
    pub fn new(proxy: EventLoopProxy<BlitzShellEvent>) -> Self {
        BlitzApplication {
            windows: HashMap::new(),
            pending_windows: Vec::new(),
            proxy,
        }
    }

    pub fn add_window(&mut self, window_config: WindowConfig<Rend>) {
        self.pending_windows.push(window_config);
    }

    fn window_mut_by_doc_id(&mut self, doc_id: usize) -> Option<&mut View<Rend>> {
        self.windows.values_mut().find(|w| w.doc.id() == doc_id)
    }
}

impl<Rend: WindowRenderer> ApplicationHandler<BlitzShellEvent> for BlitzApplication<Rend> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Resume existing windows
        for (_, view) in self.windows.iter_mut() {
            view.resume();
        }

        // Initialise pending windows
        for window_config in self.pending_windows.drain(..) {
            let mut view = View::init(window_config, event_loop, &self.proxy);
            view.resume();
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

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        // Exit the app when window close is requested.
        if matches!(event, WindowEvent::CloseRequested) {
            // Drop window before exiting event loop
            // See https://github.com/rust-windowing/winit/issues/4135
            let window = self.windows.remove(&window_id);
            drop(window);
            if self.windows.is_empty() {
                event_loop.exit();
            }
            return;
        }

        if let Some(window) = self.windows.get_mut(&window_id) {
            window.handle_winit_event(event);
        }

        let _ = self.proxy.send_event(BlitzShellEvent::Poll { window_id });
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: BlitzShellEvent) {
        match event {
            BlitzShellEvent::Poll { window_id } => {
                if let Some(window) = self.windows.get_mut(&window_id) {
                    window.poll();
                };
            }
            BlitzShellEvent::ResourceLoad { doc_id, .. } => {
                // TODO: Handle multiple documents per window
                if let Some(window) = self.window_mut_by_doc_id(doc_id) {
                    window.request_redraw();
                }
            }

            #[cfg(feature = "accessibility")]
            BlitzShellEvent::Accessibility { window_id, data } => {
                if let Some(window) = self.windows.get_mut(&window_id) {
                    match &*data {
                        accesskit_winit::WindowEvent::InitialTreeRequested => {
                            window.build_accessibility_tree();
                        }
                        accesskit_winit::WindowEvent::AccessibilityDeactivated => {
                            // TODO
                        }
                        accesskit_winit::WindowEvent::ActionRequested(_req) => {
                            // TODO
                        }
                    }
                }
            }

            BlitzShellEvent::Embedder(_) => {
                // Do nothing. Should be handled by embedders (if required).
            }
            BlitzShellEvent::Navigate(_opts) => {
                // Do nothing. Should be handled by embedders (if required).
            }
            BlitzShellEvent::NavigationLoad { .. } => {
                // Do nothing. Should be handled by embedders (if required).
            }
        }
    }
}
