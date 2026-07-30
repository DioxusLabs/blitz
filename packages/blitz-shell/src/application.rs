use crate::event::{BlitzShellEvent, BlitzShellProxy};

use anyrender::WindowRenderer;
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

#[cfg(target_os = "macos")]
use winit::platform::macos::ApplicationHandlerExtMacOS;

use crate::{View, WindowConfig};

pub struct BlitzApplication<Rend: WindowRenderer> {
    pub windows: HashMap<WindowId, View<Rend>>,
    pub pending_windows: Vec<WindowConfig<Rend>>,
    pub proxy: BlitzShellProxy,
    pub event_queue: Receiver<BlitzShellEvent>,
    #[cfg(feature = "devtools")]
    pub devtools: Option<blitz_devtools_server::DevtoolsServer>,
    /// Windows with a swallowed element-picker button press whose matching
    /// release must also be swallowed (picking ends on the press, so by
    /// release time the picker flag is already off; letting the release
    /// through would corrupt the view's pressed-buttons state)
    #[cfg(feature = "devtools")]
    picker_pressed_windows: std::collections::HashSet<WindowId>,
}

/// Adapter that gives devtools actors access to the application's documents
#[cfg(feature = "devtools")]
struct WindowsDocumentProvider<'a, Rend: WindowRenderer> {
    windows: &'a mut HashMap<WindowId, View<Rend>>,
}

#[cfg(feature = "devtools")]
impl<Rend: WindowRenderer> blitz_devtools_server::DocumentProvider
    for WindowsDocumentProvider<'_, Rend>
{
    fn document_ids(&self) -> Vec<usize> {
        self.windows.values().map(|view| view.doc.id()).collect()
    }

    fn with_document(&mut self, id: usize, cb: &mut dyn FnMut(&mut blitz_dom::BaseDocument)) {
        if let Some(view) = self.windows.values_mut().find(|view| view.doc.id() == id) {
            cb(&mut view.doc.inner_mut());
        }
    }
}

impl<Rend: WindowRenderer> BlitzApplication<Rend> {
    pub fn new(proxy: BlitzShellProxy, event_queue: Receiver<BlitzShellEvent>) -> Self {
        #[allow(unused_mut)]
        let mut app = BlitzApplication {
            windows: HashMap::new(),
            pending_windows: Vec::new(),
            proxy,
            event_queue,
            #[cfg(feature = "devtools")]
            devtools: None,
            #[cfg(feature = "devtools")]
            picker_pressed_windows: std::collections::HashSet::new(),
        };

        // Opt-in devtools server: enabled by setting the BLITZ_DEVTOOLS_PORT
        // environment variable to the port to listen on
        #[cfg(feature = "devtools")]
        if let Ok(port) = std::env::var("BLITZ_DEVTOOLS_PORT") {
            if let Ok(port) = port.parse::<u16>() {
                app.start_devtools_server(port);
            } else {
                eprintln!("Devtools: invalid BLITZ_DEVTOOLS_PORT: {port}");
            }
        }

        app
    }

    /// Start a Firefox devtools protocol server listening on localhost at
    /// the given port. Connect to it from Firefox via about:debugging.
    #[cfg(feature = "devtools")]
    pub fn start_devtools_server(&mut self, port: u16) {
        use std::sync::Arc;
        let mut server = blitz_devtools_server::DevtoolsServer::new(Arc::new(self.proxy.clone()));
        server.start_listening(&format!("127.0.0.1:{port}"));
        self.devtools = Some(server);
    }

    pub fn add_window(&mut self, window_config: WindowConfig<Rend>) {
        self.pending_windows.push(window_config);
    }

    fn window_mut_by_doc_id(&mut self, doc_id: usize) -> Option<&mut View<Rend>> {
        self.windows.values_mut().find(|w| w.doc.id() == doc_id)
    }

    /// Intercept window input events while the devtools element picker is
    /// active: mouse movement and clicks are reported to the devtools client
    /// (which highlights/selects the corresponding node) instead of being
    /// delivered to the page, and Escape cancels picking. Returns `true` if
    /// the event was consumed.
    #[cfg(feature = "devtools")]
    fn handle_picker_event(&mut self, window_id: WindowId, event: &WindowEvent) -> bool {
        use blitz_devtools_server::PickerEvent;
        use winit::event::ElementState;
        use winit::keyboard::{Key, NamedKey};

        if self.devtools.is_none() {
            return false;
        }
        if let WindowEvent::PointerButton { state, .. } = event
            && *state == ElementState::Released
            && self.picker_pressed_windows.remove(&window_id)
        {
            return true;
        }
        let Some(window) = self.windows.get(&window_id) else {
            return false;
        };
        if !window.doc.inner().devtools().element_picker {
            return false;
        }
        let doc_id = window.doc.id();

        let picker_event = match event {
            WindowEvent::PointerMoved { position, .. } => {
                let coords = window.pointer_coords(*position);
                PickerEvent::Hovered {
                    doc_id,
                    x: coords.page_x,
                    y: coords.page_y,
                }
            }
            WindowEvent::PointerButton {
                state, position, ..
            } => {
                if *state != ElementState::Pressed {
                    return true;
                }
                self.picker_pressed_windows.insert(window_id);
                let coords = window.pointer_coords(*position);
                PickerEvent::Picked {
                    doc_id,
                    x: coords.page_x,
                    y: coords.page_y,
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed
                    || event.logical_key != Key::Named(NamedKey::Escape)
                {
                    return false;
                }
                PickerEvent::Canceled { doc_id }
            }
            _ => return false,
        };

        if let Some(mut devtools) = self.devtools.take() {
            let mut provider = WindowsDocumentProvider {
                windows: &mut self.windows,
            };
            devtools.notify_picker_event(picker_event, &mut provider);
            self.devtools = Some(devtools);
        }
        true
    }

    pub fn handle_blitz_shell_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        event: BlitzShellEvent,
    ) {
        match event {
            BlitzShellEvent::Poll { window_id } => {
                if let Some(window) = self.windows.get_mut(&window_id) {
                    window.poll();
                };
            }
            BlitzShellEvent::CloseWindow { window_id } => {
                // Drop window before exiting event loop
                // See https://github.com/rust-windowing/winit/issues/4135
                let window = self.windows.remove(&window_id);
                drop(window);
                if self.windows.is_empty() {
                    event_loop.exit();
                }
            }
            BlitzShellEvent::ResumeReady { window_id } => {
                // The renderer fires `on_ready` after it has sent on the
                // channel, so `complete_resume` should always succeed here.
                // If a stale event survives a suspend, dropping it is safe.
                if let Some(window) = self.windows.get_mut(&window_id) {
                    let ok = window.complete_resume();
                    debug_assert!(ok, "ResumeReady received but renderer not ready");
                }
            }
            BlitzShellEvent::RequestRedraw { doc_id } => {
                // TODO: Handle multiple documents per window
                if let Some(window) = self.window_mut_by_doc_id(doc_id) {
                    window.request_redraw();
                }
            }

            #[cfg(feature = "accessibility")]
            BlitzShellEvent::Accessibility { window_id, data } => {
                if let Some(window) = self.windows.get_mut(&window_id) {
                    match &*data {
                        accesskit_xplat::WindowEvent::InitialTreeRequested => {
                            window.build_accessibility_tree();
                        }
                        accesskit_xplat::WindowEvent::AccessibilityDeactivated => {
                            // TODO
                        }
                        accesskit_xplat::WindowEvent::ActionRequested(_req) => {
                            // TODO
                        }
                    }
                }
            }
            #[cfg(feature = "devtools")]
            BlitzShellEvent::DevtoolsPoll => {
                if let Some(mut devtools) = self.devtools.take() {
                    let mut provider = WindowsDocumentProvider {
                        windows: &mut self.windows,
                    };
                    devtools.process_messages(&mut provider);
                    self.devtools = Some(devtools);
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
            #[cfg(target_arch = "wasm32")]
            BlitzShellEvent::ResizeSettleCheck { window_id } => {
                if let Some(window) = self.windows.get_mut(&window_id) {
                    window.apply_pending_resize_if_settled();
                }
            }
        }
    }
}

impl<Rend: WindowRenderer> ApplicationHandler for BlitzApplication<Rend> {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        // Resume existing windows
        for view in self.windows.values_mut() {
            view.resume();
        }

        // Initialise pending windows. The renderer's resume is non-blocking —
        // on native it finishes inline, on wasm32 it spawns a future that will
        // dispatch BlitzShellEvent::ResumeReady when init completes. Either way
        // we insert the view immediately so the event handler can find it.
        for window_config in self.pending_windows.drain(..) {
            let mut view = View::init(window_config, event_loop, &self.proxy);
            view.resume();
            self.windows.insert(view.window_id(), view);
        }
    }

    fn destroy_surfaces(&mut self, _event_loop: &dyn ActiveEventLoop) {
        for view in self.windows.values_mut() {
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

        #[cfg(feature = "devtools")]
        if self.handle_picker_event(window_id, &event) {
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

    #[cfg(target_os = "macos")]
    fn macos_handler(&mut self) -> Option<&mut dyn ApplicationHandlerExtMacOS> {
        Some(self)
    }

    #[cfg(target_os = "ios")]
    fn about_to_wait(&mut self, _event_loop: &dyn ActiveEventLoop) {
        for view in self.windows.values_mut() {
            if view.ios_request_redraw.get() {
                view.window.request_redraw();
            }
        }
    }
}

#[cfg(target_os = "macos")]
impl<Rend: WindowRenderer> ApplicationHandlerExtMacOS for BlitzApplication<Rend> {
    fn standard_key_binding(
        &mut self,
        _event_loop: &dyn ActiveEventLoop,
        window_id: WindowId,
        action: &str,
    ) {
        if let Some(window) = self.windows.get_mut(&window_id) {
            window.handle_apple_standard_keybinding(action);
            self.proxy.send_event(BlitzShellEvent::Poll { window_id });
        }
    }
}
