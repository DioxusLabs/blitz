//! Dioxus integration for UIKit renderer
//!
//! This module provides a `launch` function similar to `dioxus-native` that:
//! - Creates a VirtualDom from a Dioxus component
//! - Wraps it in a DioxusDocument
//! - Renders to native UIKit views
//! - Handles events and async tasks

use crate::application::{create_waker, UIKitEvent, UIKitProxy};
use crate::events::{drain_input_events, has_pending_input_events, InputEvent};
use crate::UIKitRenderer;

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::task::{Context as TaskContext, Waker};

use blitz_dom::Document;
use blitz_traits::events::{
    BlitzFocusEvent, BlitzInputEvent, BlitzPointerId, BlitzPointerEvent, DomEvent, DomEventData,
    MouseEventButton, MouseEventButtons, UiEvent,
};
use blitz_traits::shell::{ColorScheme, ShellProvider, Viewport};
use dioxus_core::{ComponentFunction, Element, VirtualDom};
use dioxus_native_dom::{DioxusDocument, DocumentConfig};
use keyboard_types::Modifiers;
use objc2::rc::Retained;
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use objc2_ui_kit::UIView;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

// =============================================================================
// UIKitShellProvider - for triggering redraws when resources load
// =============================================================================

/// Shell provider that triggers window redraws when resources (images, fonts) load.
pub struct UIKitShellProvider {
    window: Arc<dyn Window>,
}

impl UIKitShellProvider {
    pub fn new(window: Arc<dyn Window>) -> Self {
        Self { window }
    }
}

impl ShellProvider for UIKitShellProvider {
    fn request_redraw(&self) {
        self.window.request_redraw();
    }
}

// =============================================================================
// DioxusUIKitView
// =============================================================================

/// A view that renders a DioxusDocument using UIKit.
///
/// This is the Dioxus-aware version of UIKitView. It polls the VirtualDom
/// and handles events through the Dioxus event system.
pub struct DioxusUIKitView {
    /// The winit window
    window: Arc<dyn Window>,
    /// The Dioxus document (wraps VirtualDom + BaseDocument)
    doc: DioxusDocument,
    /// The UIKit renderer
    renderer: UIKitRenderer,
    /// Waker for async integration
    waker: Option<Waker>,
    /// Proxy for event loop communication
    proxy: UIKitProxy,
    /// MainThreadMarker for UIKit operations
    mtm: MainThreadMarker,
    /// Whether a redraw is needed
    needs_redraw: Cell<bool>,
    /// Whether the view has been initialized
    initialized: Cell<bool>,
}

impl DioxusUIKitView {
    /// Initialize a new Dioxus view.
    pub fn init(
        mut doc: DioxusDocument,
        attributes: WindowAttributes,
        event_loop: &dyn ActiveEventLoop,
        proxy: &UIKitProxy,
    ) -> Self {
        let mtm = MainThreadMarker::new().expect("DioxusUIKitView must be created on main thread");

        // Create window
        let window: Arc<dyn Window> = Arc::from(
            event_loop.create_window(attributes).unwrap()
        );

        // Get window metrics
        let size = window.surface_size();
        let scale = window.scale_factor() as f32;

        // Set up shell provider for resource loading callbacks (images, fonts)
        let shell_provider = Arc::new(UIKitShellProvider::new(window.clone()));
        doc.inner.borrow_mut().set_shell_provider(shell_provider);

        // Set viewport on document
        let viewport = Viewport::new(size.width, size.height, scale, ColorScheme::Light);
        doc.inner.borrow_mut().set_viewport(viewport);

        // Get root UIView from window
        let root_view = get_uiview_from_window(&*window, mtm);

        // Create renderer using the inner BaseDocument
        let mut renderer = UIKitRenderer::new(doc.inner.clone(), root_view, mtm);
        renderer.set_scale(scale as f64);

        // Create waker
        let waker = create_waker(proxy, window.id());

        // Run initial build of the VirtualDom
        doc.initial_build();

        Self {
            window,
            doc,
            renderer,
            waker: Some(waker),
            proxy: proxy.clone(),
            mtm,
            needs_redraw: Cell::new(true),
            initialized: Cell::new(false),
        }
    }

    /// Get the window ID.
    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    /// Get the document ID.
    pub fn doc_id(&self) -> usize {
        self.doc.id()
    }

    /// Check if the view needs a redraw.
    pub fn needs_redraw(&self) -> bool {
        self.needs_redraw.get()
    }

    /// Request a redraw of this view.
    pub fn request_redraw(&self) {
        self.needs_redraw.set(true);
        self.window.request_redraw();
    }

    /// Resume rendering (called when surface is available).
    pub fn resume(&mut self) {
        if !self.initialized.get() {
            self.doc.inner.borrow_mut().resolve(0.0);
            self.renderer.sync();
            self.initialized.set(true);
            self.needs_redraw.set(false);
        }
    }

    /// Suspend rendering (called when surface is lost).
    pub fn suspend(&mut self) {
        self.waker = None;
    }

    /// Poll the VirtualDom for updates.
    ///
    /// Returns true if there were changes that require a redraw.
    pub fn poll(&mut self) -> bool {
        // Create task context from waker
        let cx = self.waker.as_ref().map(|w| TaskContext::from_waker(w));

        // Poll the DioxusDocument (this drives the VirtualDom)
        let has_changes = self.doc.poll(cx);

        if has_changes {
            self.request_redraw();
        }

        has_changes
    }

    /// Process queued input events from UIKit native controls.
    ///
    /// This converts InputEvents from native UIKit controls (UITextField, etc.)
    /// to DomEvents and dispatches them through the Dioxus event system.
    ///
    /// Returns true if any events were processed.
    pub fn process_input_events(&mut self) -> bool {
        let events = drain_input_events();
        let had_events = !events.is_empty();

        for event in events {
            let dom_event = match event {
                InputEvent::Click { node_id } => {
                    DomEvent::new(
                        node_id,
                        DomEventData::Click(BlitzPointerEvent {
                            id: BlitzPointerId::Finger(0),
                            is_primary: true,
                            x: 0.0,
                            y: 0.0,
                            screen_x: 0.0,
                            screen_y: 0.0,
                            client_x: 0.0,
                            client_y: 0.0,
                            button: MouseEventButton::Main,
                            buttons: MouseEventButtons::None,
                            mods: Modifiers::empty(),
                        }),
                    )
                }
                InputEvent::TextChanged { node_id, value } => {
                    DomEvent::new(node_id, DomEventData::Input(BlitzInputEvent { value }))
                }
                InputEvent::FocusGained { node_id } => {
                    DomEvent::new(node_id, DomEventData::Focus(BlitzFocusEvent))
                }
                InputEvent::FocusLost { node_id } => {
                    DomEvent::new(node_id, DomEventData::Blur(BlitzFocusEvent))
                }
            };
            // Dispatch through Dioxus event handler
            self.doc.handle_dom_event(dom_event);
        }

        had_events
    }

    /// Perform a redraw if needed.
    pub fn redraw(&mut self) {
        if !self.needs_redraw.get() {
            return;
        }

        self.needs_redraw.set(false);

        // Process any queued input events
        let had_events = self.process_input_events();

        // If we had events, poll the VirtualDom to process them and get mutations
        if had_events {
            // Poll with no waker since we're already in redraw
            let _ = self.doc.poll(None);
        }

        // Resolve layout
        self.doc.inner.borrow_mut().resolve(0.0);

        // Sync UIKit views with DOM
        self.renderer.sync();
    }

    /// Handle a winit window event.
    pub fn handle_window_event(&mut self, event: WindowEvent) {
        match event {
            WindowEvent::RedrawRequested => {
                self.redraw();
            }
            WindowEvent::SurfaceResized(size) => {
                let scale = self.window.scale_factor() as f32;
                let viewport = Viewport::new(size.width, size.height, scale, ColorScheme::Light);
                self.doc.inner.borrow_mut().set_viewport(viewport);
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.renderer.set_scale(scale_factor);
                let mut inner = self.doc.inner.borrow_mut();
                let (width, height) = inner.viewport().window_size;
                let viewport = Viewport::new(width, height, scale_factor as f32, ColorScheme::Light);
                inner.set_viewport(viewport);
                drop(inner);
                self.request_redraw();
            }
            WindowEvent::PointerMoved { position, primary, .. } => {
                let scale = self.window.scale_factor();
                let x = (position.x / scale) as f32;
                let y = (position.y / scale) as f32;

                let event = UiEvent::MouseMove(BlitzPointerEvent {
                    id: BlitzPointerId::Finger(0),
                    is_primary: primary,
                    x,
                    y,
                    screen_x: x,
                    screen_y: y,
                    client_x: x,
                    client_y: y,
                    button: MouseEventButton::Main,
                    buttons: MouseEventButtons::Primary,
                    mods: Modifiers::empty(),
                });

                self.doc.handle_ui_event(event);
            }
            WindowEvent::PointerButton { state, position, primary, .. } => {
                let scale = self.window.scale_factor();
                let x = (position.x / scale) as f32;
                let y = (position.y / scale) as f32;

                let (event_type, buttons) = match state {
                    ElementState::Pressed => (true, MouseEventButtons::Primary),
                    ElementState::Released => (false, MouseEventButtons::None),
                };

                let pointer_event = BlitzPointerEvent {
                    id: BlitzPointerId::Finger(0),
                    is_primary: primary,
                    x,
                    y,
                    screen_x: x,
                    screen_y: y,
                    client_x: x,
                    client_y: y,
                    button: MouseEventButton::Main,
                    buttons,
                    mods: Modifiers::empty(),
                };

                let event = if event_type {
                    println!("[DioxusUIKitView] Pointer down at ({}, {})", x, y);
                    UiEvent::MouseDown(pointer_event)
                } else {
                    println!("[DioxusUIKitView] Pointer up at ({}, {})", x, y);
                    UiEvent::MouseUp(pointer_event)
                };

                self.doc.handle_ui_event(event);
                self.request_redraw();
            }
            _ => {}
        }
    }
}

/// Extract the root UIView from a winit window.
fn get_uiview_from_window(window: &dyn Window, mtm: MainThreadMarker) -> Retained<UIView> {
    let handle = window.window_handle().expect("Failed to get window handle");
    let raw_handle = handle.as_raw();

    match raw_handle {
        RawWindowHandle::UiKit(uikit_handle) => {
            let ui_view_ptr = uikit_handle.ui_view.as_ptr();
            unsafe {
                let ui_view: *mut UIView = ui_view_ptr.cast();
                Retained::retain(ui_view).expect("UIView pointer should be valid")
            }
        }
        _ => {
            eprintln!("[WARNING] Not a UIKit window handle, creating detached UIView");
            let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(390.0, 844.0));
            UIView::initWithFrame(mtm.alloc::<UIView>(), frame)
        }
    }
}

// =============================================================================
// DioxusUIKitApplication
// =============================================================================

/// Application handler for Dioxus UIKit apps.
pub struct DioxusUIKitApplication {
    /// Active views by window ID
    views: HashMap<WindowId, DioxusUIKitView>,
    /// Pending document to create on resume
    pending_doc: Option<(DioxusDocument, WindowAttributes)>,
    /// Proxy for sending events
    proxy: UIKitProxy,
    /// Receiver for application events
    event_queue: Receiver<UIKitEvent>,
}

impl DioxusUIKitApplication {
    /// Create a new application.
    pub fn new(
        doc: DioxusDocument,
        attributes: WindowAttributes,
        proxy: UIKitProxy,
        event_queue: Receiver<UIKitEvent>,
    ) -> Self {
        Self {
            views: HashMap::new(),
            pending_doc: Some((doc, attributes)),
            proxy,
            event_queue,
        }
    }

    fn view_by_doc_id(&mut self, doc_id: usize) -> Option<&mut DioxusUIKitView> {
        self.views.values_mut().find(|v| v.doc_id() == doc_id)
    }

    fn handle_event(&mut self, _event_loop: &dyn ActiveEventLoop, event: UIKitEvent) {
        match event {
            UIKitEvent::Poll { window_id } => {
                if let Some(view) = self.views.get_mut(&window_id) {
                    view.poll();
                }
            }
            UIKitEvent::RequestRedraw { doc_id } => {
                if let Some(view) = self.view_by_doc_id(doc_id) {
                    view.request_redraw();
                }
            }
        }
    }
}

impl ApplicationHandler for DioxusUIKitApplication {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        // Resume existing views
        for view in self.views.values_mut() {
            view.resume();
        }

        // Create pending view
        if let Some((doc, attributes)) = self.pending_doc.take() {
            let view = DioxusUIKitView::init(doc, attributes, event_loop, &self.proxy);
            self.views.insert(view.window_id(), view);
        }
    }

    fn destroy_surfaces(&mut self, _event_loop: &dyn ActiveEventLoop) {
        for view in self.views.values_mut() {
            view.suspend();
        }
    }

    fn resumed(&mut self, _event_loop: &dyn ActiveEventLoop) {
        for view in self.views.values_mut() {
            view.request_redraw();
        }
    }

    fn suspended(&mut self, _event_loop: &dyn ActiveEventLoop) {}

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if matches!(event, WindowEvent::CloseRequested) {
            self.views.remove(&window_id);
            if self.views.is_empty() {
                event_loop.exit();
            }
            return;
        }

        if let Some(view) = self.views.get_mut(&window_id) {
            view.handle_window_event(event);
        }

        self.proxy.send_event(UIKitEvent::Poll { window_id });
    }

    fn proxy_wake_up(&mut self, event_loop: &dyn ActiveEventLoop) {
        while let Ok(event) = self.event_queue.try_recv() {
            self.handle_event(event_loop, event);
        }
    }

    fn about_to_wait(&mut self, _event_loop: &dyn ActiveEventLoop) {
        // Check if there are pending input events from native controls (buttons, text fields)
        let has_pending = has_pending_input_events();

        for view in self.views.values() {
            // Request redraw if view needs it OR if there are pending input events
            if view.needs_redraw() || has_pending {
                view.request_redraw();
            }
        }
    }
}

// =============================================================================
// Launch function
// =============================================================================

/// Launch a Dioxus app with UIKit rendering.
///
/// This is the main entry point for Dioxus apps on iOS using native UIKit views.
///
/// # Example
///
/// ```ignore
/// use blitz_ios_uikit::launch;
/// use dioxus::prelude::*;
///
/// fn app() -> Element {
///     rsx! {
///         div {
///             h1 { "Hello, iOS!" }
///             button {
///                 onclick: |_| println!("Clicked!"),
///                 "Click me"
///             }
///         }
///     }
/// }
///
/// fn main() {
///     launch(app);
/// }
/// ```
pub fn launch(app: fn() -> Element) {
    launch_cfg(app, WindowAttributes::default())
}

/// Launch a Dioxus app with custom window configuration.
pub fn launch_cfg(app: fn() -> Element, attributes: WindowAttributes) {
    launch_cfg_with_props(app, (), attributes)
}

/// Launch a Dioxus app with props and custom window configuration.
pub fn launch_cfg_with_props<P: Clone + 'static, M: 'static>(
    app: impl ComponentFunction<P, M>,
    props: P,
    attributes: WindowAttributes,
) {
    // Create tokio runtime for async networking
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");
    let _guard = rt.enter();

    // Create net provider for image/font loading
    let net_provider = blitz_net::Provider::shared(None);

    // Create event loop
    let event_loop = EventLoop::new().expect("Failed to create event loop");
    let winit_proxy = event_loop.create_proxy();
    let (proxy, event_queue) = UIKitProxy::new(winit_proxy);

    // Create VirtualDom
    let vdom = VirtualDom::new_with_props(app, props);

    // Create DioxusDocument with net provider
    let config = DocumentConfig {
        net_provider: Some(net_provider),
        ..Default::default()
    };
    let doc = DioxusDocument::new(vdom, config);

    // Create application
    let application = DioxusUIKitApplication::new(doc, attributes, proxy, event_queue);

    // Run event loop
    event_loop.run_app(application).expect("Event loop failed");
}
