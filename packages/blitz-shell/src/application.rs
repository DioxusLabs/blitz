use crate::event::{BlitzShellEvent, BlitzShellProxy};

use anyrender::WindowRenderer;
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::{View, WindowConfig};

pub struct BlitzApplication<Rend: WindowRenderer> {
    pub windows: HashMap<WindowId, View<Rend>>,
    pub pending_windows: Vec<WindowConfig<Rend>>,
    pub proxy: BlitzShellProxy,
    pub event_queue: Receiver<BlitzShellEvent>,
}

impl<Rend: WindowRenderer> BlitzApplication<Rend> {
    pub fn new(proxy: BlitzShellProxy, event_queue: Receiver<BlitzShellEvent>) -> Self {
        BlitzApplication {
            windows: HashMap::new(),
            pending_windows: Vec::new(),
            proxy,
            event_queue,
        }
    }

    pub fn add_window(&mut self, window_config: WindowConfig<Rend>) {
        self.pending_windows.push(window_config);
    }

    fn window_mut_by_doc_id(&mut self, doc_id: usize) -> Option<&mut View<Rend>> {
        self.windows.values_mut().find(|w| w.doc.id() == doc_id)
    }

    pub fn handle_blitz_shell_event(
        &mut self,
        _event_loop: &dyn ActiveEventLoop,
        event: BlitzShellEvent,
    ) {
        match event {
            BlitzShellEvent::Poll { window_id } => {
                if let Some(window) = self.windows.get_mut(&window_id) {
                    window.poll();
                };
            }
            BlitzShellEvent::RequestRedraw { doc_id } => {
                // TODO: Handle multiple documents per window
                if let Some(window) = self.window_mut_by_doc_id(doc_id) {
                    window.request_redraw();
                }
            }

            // #[cfg(feature = "accessibility")]
            // BlitzShellEvent::Accessibility { window_id, data } => {
            //     if let Some(window) = self.windows.get_mut(&window_id) {
            //         match &*data {
            //             accesskit_winit::WindowEvent::InitialTreeRequested => {
            //                 window.build_accessibility_tree();
            //             }
            //             accesskit_winit::WindowEvent::AccessibilityDeactivated => {
            //                 // TODO
            //             }
            //             accesskit_winit::WindowEvent::ActionRequested(_req) => {
            //                 // TODO
            //             }
            //         }
            //     }
            // }
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

impl<Rend: WindowRenderer> ApplicationHandler for BlitzApplication<Rend> {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
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

    fn destroy_surfaces(&mut self, _event_loop: &dyn ActiveEventLoop) {
        for (_, view) in self.windows.iter_mut() {
            view.suspend();
        }
    }

    fn resumed(&mut self, _event_loop: &dyn ActiveEventLoop) {
        // TODO
    }

    fn suspended(&mut self, _event_loop: &dyn ActiveEventLoop) {
        // TODO
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
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

        self.proxy.send_event(BlitzShellEvent::Poll { window_id });
    }

    fn proxy_wake_up(&mut self, event_loop: &dyn ActiveEventLoop) {
        while let Ok(event) = self.event_queue.try_recv() {
            self.handle_blitz_shell_event(event_loop, event);
        }
    }
}
