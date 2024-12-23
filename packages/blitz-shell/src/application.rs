use crate::event::BlitzEvent;

use blitz_dom::{DocumentLike, DocumentRenderer};
use std::collections::HashMap;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::WindowId;

use crate::{View, WindowConfig};

pub struct BlitzApplication<Doc: DocumentLike, Rend: DocumentRenderer> {
    pub windows: HashMap<WindowId, View<Doc, Rend>>,
    pending_windows: Vec<WindowConfig<Doc, Rend>>,
    proxy: EventLoopProxy<BlitzEvent>,

    #[cfg(all(feature = "menu", not(any(target_os = "android", target_os = "ios"))))]
    menu_channel: muda::MenuEventReceiver,
}

impl<Doc: DocumentLike, Rend: DocumentRenderer> BlitzApplication<Doc, Rend> {
    pub fn new(proxy: EventLoopProxy<BlitzEvent>) -> Self {
        BlitzApplication {
            windows: HashMap::new(),
            pending_windows: Vec::new(),
            proxy,

            #[cfg(all(feature = "menu", not(any(target_os = "android", target_os = "ios"))))]
            menu_channel: muda::MenuEvent::receiver().clone(),
        }
    }

    pub fn add_window(&mut self, window_config: WindowConfig<Doc, Rend>) {
        self.pending_windows.push(window_config);
    }

    fn window_mut_by_doc_id(&mut self, doc_id: usize) -> Option<&mut View<Doc, Rend>> {
        self.windows.values_mut().find(|w| w.doc.id() == doc_id)
    }
}

impl<Doc: DocumentLike, Rend: DocumentRenderer> ApplicationHandler<BlitzEvent>
    for BlitzApplication<Doc, Rend>
{
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

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, _cause: winit::event::StartCause) {
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

        let _ = self.proxy.send_event(BlitzEvent::Poll { window_id });
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: BlitzEvent) {
        match event {
            BlitzEvent::Poll { window_id } => {
                if let Some(window) = self.windows.get_mut(&window_id) {
                    window.poll();
                };
            }

            BlitzEvent::ResourceLoad { doc_id, data } => {
                // TODO: Handle multiple documents per window
                if let Some(window) = self.window_mut_by_doc_id(doc_id) {
                    window.doc.as_mut().load_resource(data);
                    window.request_redraw();
                }
            }

            #[cfg(feature = "accessibility")]
            BlitzEvent::Accessibility { window_id, data } => {
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

            BlitzEvent::Embedder(_) => {
                // Do nothing. Should be handled by embedders (if required).
            }
        }
    }
}
