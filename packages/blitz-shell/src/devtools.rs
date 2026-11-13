//! Chrome DevTools Protocol integration (the `devtools` feature): starting
//! the CDP server, giving it access to the application's documents, and
//! routing element-picker input to it.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use anyrender::WindowRenderer;
use blitz_vibey_cdp_server::{CdpServer, DevtoolsWaker, DocumentProvider, PickerEvent, documents};
use webbrowser::Browser;
use winit::event::{ElementState, WindowEvent};
use winit::keyboard::{Key, NamedKey};
use winit::window::WindowId;

use crate::{BlitzApplication, BlitzShellEvent, BlitzShellProxy, View};

#[derive(Default)]
pub(crate) struct DevtoolsState {
    cdp: Option<CdpServer>,
    /// Windows with a swallowed element-picker button press whose matching
    /// release must also be swallowed (picking ends on the press, so by
    /// release time the picker flag is already off; letting the release
    /// through would corrupt the view's pressed-buttons state)
    picker_pressed_windows: HashSet<WindowId>,
}

impl DevtoolsWaker for BlitzShellProxy {
    fn wake(&self) {
        self.send_event(BlitzShellEvent::DevtoolsPoll)
    }
}

/// Adapter that gives the CDP server access to the application's documents
struct WindowsDocumentProvider<'a, Rend: WindowRenderer> {
    windows: &'a mut HashMap<WindowId, View<Rend>>,
}

impl<Rend: WindowRenderer> DocumentProvider for WindowsDocumentProvider<'_, Rend> {
    fn document_ids(&self) -> Vec<usize> {
        let mut ids = Vec::new();
        for view in self.windows.values() {
            documents::collect_document_ids(&view.doc.inner(), &mut ids);
        }
        ids
    }

    fn with_document(&mut self, id: usize, cb: &mut dyn FnMut(&mut blitz_dom::BaseDocument)) {
        for view in self.windows.values_mut() {
            if documents::with_document_in(&mut view.doc.inner_mut(), id, cb) {
                return;
            }
        }
    }
}

impl<Rend: WindowRenderer> BlitzApplication<Rend> {
    /// Opt-in Chrome DevTools Protocol server: enabled by setting the
    /// BLITZ_CDP_PORT environment variable to the port to listen on
    pub(crate) fn start_cdp_server_from_env(&mut self) {
        if let Ok(port) = std::env::var("BLITZ_CDP_PORT") {
            if let Ok(port) = port.parse::<u16>() {
                self.start_cdp_server(port);
            } else {
                eprintln!("CDP: invalid BLITZ_CDP_PORT: {port}");
            }
        }
    }

    /// Start a Chrome DevTools Protocol server listening on localhost at
    /// the given port (`0` picks a free port). Connect to it with
    /// [`open_devtools`](Self::open_devtools), or by opening
    /// `devtools://devtools/bundled/devtools_app.html?ws=127.0.0.1:PORT/devtools/page/DOC_ID`
    /// in Chrome, or via chrome://inspect (note: chrome://inspect opens the
    /// "screencast" devtools app, which routes element picking and node
    /// highlighting to its screencast pane — toggle the screencast off to
    /// use them on the Blitz window directly).
    ///
    /// Note: the address must be `127.0.0.1`, not `localhost` — the devtools
    /// frontend's Content Security Policy only allows `ws://127.0.0.1:*`
    /// connections and silently blocks `localhost`.
    pub fn start_cdp_server(&mut self, port: u16) {
        let mut server = CdpServer::new(Arc::new(self.proxy.clone()));
        server.start_listening(&format!("127.0.0.1:{port}"));
        self.devtools.cdp = Some(server);
    }

    /// Open a Chrome DevTools frontend inspecting the document with the given
    /// id (in Chrome if it is installed, otherwise in the default browser),
    /// first starting the CDP server on a free port if it isn't running. If
    /// the CDP server doesn't embed a frontend, open Chrome's bundled frontend
    /// instead, which only Chrome can open (and only where it can be launched
    /// directly, e.g. not on Linux), printing its URL if it can't be.
    pub fn open_devtools(&mut self, doc_id: usize) {
        if self.devtools.cdp.is_none() {
            self.start_cdp_server(0);
        }
        // Inspect the document's target, which for a sub-document (e.g. a
        // browser tab) follows its host element across navigations
        let mut target_id = None;
        self.with_cdp(|_, docs| target_id = blitz_vibey_cdp_server::target_id(docs, doc_id));
        let Some(target_id) = target_id else {
            eprintln!("CDP: can't open devtools: no document {doc_id}");
            return;
        };
        let cdp = self.devtools.cdp.as_ref().unwrap();
        let Some(addr) = cdp.wait_for_local_addr(Duration::from_secs(1)) else {
            eprintln!("CDP: can't open devtools: the server isn't listening");
            return;
        };
        let url = blitz_vibey_cdp_server::frontend_url(addr, &target_id);
        // Chrome's bundled frontend: only Chrome can open it
        if url.starts_with("devtools://") {
            if webbrowser::open_browser(Browser::Chrome, &url).is_err() {
                eprintln!("CDP: to open devtools, paste this URL into Chrome: {url}");
            }
            return;
        }
        if webbrowser::open_browser(Browser::Chrome, &url).is_err()
            && let Err(err) = webbrowser::open(&url)
        {
            eprintln!("CDP: failed to open devtools at {url}: {err}");
        }
    }

    /// Run `cb` with the CDP server (if started) and a provider for the
    /// application's documents
    fn with_cdp(&mut self, cb: impl FnOnce(&mut CdpServer, &mut dyn DocumentProvider)) {
        if let Some(cdp) = self.devtools.cdp.as_mut() {
            let mut provider = WindowsDocumentProvider {
                windows: &mut self.windows,
            };
            cb(cdp, &mut provider);
        }
    }

    /// Process pending CDP messages (on [`BlitzShellEvent::DevtoolsPoll`])
    pub(crate) fn process_devtools_messages(&mut self) {
        self.with_cdp(|cdp, provider| cdp.process_messages(provider));
    }

    /// Intercept window input events while the devtools element picker is
    /// active: mouse movement and clicks are reported to the devtools client
    /// (which highlights/selects the corresponding node) instead of being
    /// delivered to the page, and Escape cancels picking. Returns `true` if
    /// the event was consumed.
    pub(crate) fn handle_picker_event(&mut self, window_id: WindowId, event: &WindowEvent) -> bool {
        if self.devtools.cdp.is_none() {
            return false;
        }
        if let WindowEvent::PointerButton { state, .. } = event
            && *state == ElementState::Released
            && self.devtools.picker_pressed_windows.remove(&window_id)
        {
            return true;
        }
        let Some(window) = self.windows.get(&window_id) else {
            return false;
        };
        // The picker may be active on the window's document or on one of its
        // sub-documents (each an inspectable target of its own): find it and
        // translate window coordinates into its coordinate space
        let Some(picking_doc_id) = documents::picking_document_id(&window.doc.inner()) else {
            return false;
        };
        // Pointer events outside the picking document (e.g. over the browser
        // toolbar around a sub-document) are delivered as usual
        let find_picking =
            |x: f32, y: f32| documents::find_picking_document(&window.doc.inner(), x, y);

        let picker_event = match event {
            WindowEvent::PointerMoved { position, .. } => {
                let coords = window.pointer_coords(*position);
                let Some((doc_id, x, y)) = find_picking(coords.page_x, coords.page_y) else {
                    return false;
                };
                PickerEvent::Hovered { doc_id, x, y }
            }
            WindowEvent::PointerButton {
                state, position, ..
            } => {
                if *state != ElementState::Pressed {
                    return true;
                }
                let coords = window.pointer_coords(*position);
                let Some((doc_id, x, y)) = find_picking(coords.page_x, coords.page_y) else {
                    return false;
                };
                self.devtools.picker_pressed_windows.insert(window_id);
                PickerEvent::Picked { doc_id, x, y }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed
                    || event.logical_key != Key::Named(NamedKey::Escape)
                {
                    return false;
                }
                PickerEvent::Canceled {
                    doc_id: picking_doc_id,
                }
            }
            _ => return false,
        };

        self.with_cdp(|cdp, provider| cdp.notify_picker_event(picker_event, provider));
        true
    }
}
