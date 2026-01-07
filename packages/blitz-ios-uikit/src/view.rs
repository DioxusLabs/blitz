//! UIKitView - manages a document and its UIKit rendering
//!
//! This is the iOS equivalent of blitz-shell's View struct. It wraps a document,
//! renderer, and waker to provide proper async integration with the event loop.

use crate::application::{UIKitProxy, create_waker};
use crate::events::{drain_input_events, input_event_to_dom_event};
use crate::UIKitRenderer;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::task::Waker;

use blitz_dom::{BaseDocument, Document, EventDriver, NoopEventHandler};
use blitz_traits::events::{BlitzPointerId, BlitzPointerEvent, MouseEventButton, MouseEventButtons, UiEvent};
use blitz_traits::shell::{ColorScheme, Viewport};
use keyboard_types::Modifiers;
use objc2::rc::Retained;
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use objc2_ui_kit::UIView;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes, WindowId};

// =============================================================================
// ViewConfig
// =============================================================================

/// Configuration for creating a UIKitView.
pub struct ViewConfig {
    /// The document to render
    pub doc: Rc<RefCell<BaseDocument>>,
    /// Window attributes
    pub attributes: WindowAttributes,
}

impl ViewConfig {
    /// Create a new view configuration.
    pub fn new(doc: Rc<RefCell<BaseDocument>>) -> Self {
        Self {
            doc,
            attributes: WindowAttributes::default(),
        }
    }

    /// Create a view configuration with custom window attributes.
    pub fn with_attributes(doc: Rc<RefCell<BaseDocument>>, attributes: WindowAttributes) -> Self {
        Self { doc, attributes }
    }
}

// =============================================================================
// UIKitView
// =============================================================================

/// A view that renders a document using UIKit.
///
/// This manages the window, document, renderer, and async waker integration.
pub struct UIKitView {
    /// The winit window
    window: Arc<dyn Window>,
    /// The document being rendered
    doc: Rc<RefCell<BaseDocument>>,
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

impl UIKitView {
    /// Initialize a new view from configuration.
    pub fn init(
        config: ViewConfig,
        event_loop: &dyn ActiveEventLoop,
        proxy: &UIKitProxy,
    ) -> Self {
        let mtm = MainThreadMarker::new().expect("UIKitView must be created on main thread");

        // Create window
        let window: Arc<dyn Window> = Arc::from(
            event_loop.create_window(config.attributes).unwrap()
        );

        // Get window metrics
        let size = window.surface_size();
        let scale = window.scale_factor() as f32;

        // Set viewport on document
        let viewport = Viewport::new(size.width, size.height, scale, ColorScheme::Light);
        config.doc.borrow_mut().set_viewport(viewport);

        // Get root UIView from window
        let root_view = get_uiview_from_window(&*window, mtm);

        // Create renderer
        let mut renderer = UIKitRenderer::new(config.doc.clone(), root_view, mtm);
        renderer.set_scale(scale as f64);

        // Create waker
        let waker = create_waker(proxy, window.id());

        Self {
            window,
            doc: config.doc,
            renderer,
            waker: Some(waker),
            proxy: proxy.clone(),
            mtm,
            needs_redraw: Cell::new(true), // Initial render needed
            initialized: Cell::new(false),
        }
    }

    /// Get the window ID.
    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    /// Get the document ID.
    pub fn doc_id(&self) -> usize {
        self.doc.borrow().id()
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
            // Initial sync
            self.doc.borrow_mut().resolve(0.0);
            self.renderer.sync();
            self.initialized.set(true);
            self.needs_redraw.set(false);
        }
    }

    /// Suspend rendering (called when surface is lost).
    pub fn suspend(&mut self) {
        self.waker = None;
    }

    /// Poll the document for updates.
    ///
    /// Returns true if the document had changes that require a redraw.
    ///
    /// For HtmlDocument/BaseDocument, this always returns false since there's
    /// no async work to poll. DioxusDocument would override this to poll the VirtualDom.
    pub fn poll(&mut self) -> bool {
        // For now, BaseDocument doesn't have async work to poll.
        // When we integrate with DioxusDocument, we'll use the Document trait's poll method.
        // Network messages are handled automatically in resolve() via handle_messages().
        false
    }

    /// Process queued input events from UIKit native controls.
    pub fn process_input_events(&mut self) {
        let events = drain_input_events();
        for event in events {
            let dom_event = input_event_to_dom_event(event);
            // Dispatch through the document's event system
            let mut doc = self.doc.borrow_mut();
            let mut driver = EventDriver::new(&mut *doc, NoopEventHandler);
            driver.handle_dom_event(dom_event);
        }
    }

    /// Perform a redraw if needed.
    pub fn redraw(&mut self) {
        if !self.needs_redraw.get() {
            return;
        }

        self.needs_redraw.set(false);

        // Process any queued input events
        self.process_input_events();

        // Resolve layout
        self.doc.borrow_mut().resolve(0.0);

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
                self.doc.borrow_mut().set_viewport(viewport);
                self.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.renderer.set_scale(scale_factor);
                let mut doc = self.doc.borrow_mut();
                let (width, height) = doc.viewport().window_size;
                let viewport = Viewport::new(width, height, scale_factor as f32, ColorScheme::Light);
                doc.set_viewport(viewport);
                drop(doc);
                self.request_redraw();
            }
            // Handle pointer/touch moved
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

                self.dispatch_event(event);
            }
            // Handle pointer/touch button (down/up)
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
                    println!("[UIKitView] Pointer down at ({}, {})", x, y);
                    UiEvent::MouseDown(pointer_event)
                } else {
                    println!("[UIKitView] Pointer up at ({}, {})", x, y);
                    UiEvent::MouseUp(pointer_event)
                };

                self.dispatch_event(event);
                self.request_redraw();
            }
            _ => {
                // Other events (keyboard, etc.) can be added here
            }
        }
    }

    /// Dispatch a UI event to the document's event handler.
    fn dispatch_event(&mut self, event: UiEvent) {
        let mut doc = self.doc.borrow_mut();
        let mut driver = EventDriver::new(&mut *doc, NoopEventHandler);
        driver.handle_ui_event(event);
    }
}

/// Extract the root UIView from a winit window.
fn get_uiview_from_window(window: &dyn Window, mtm: MainThreadMarker) -> Retained<UIView> {
    let handle = window.window_handle().expect("Failed to get window handle");
    let raw_handle = handle.as_raw();

    match raw_handle {
        RawWindowHandle::UiKit(uikit_handle) => {
            let ui_view_ptr = uikit_handle.ui_view.as_ptr();
            // SAFETY: The pointer comes from winit's window handle and is valid.
            // We're on the main thread (verified by MainThreadMarker).
            unsafe {
                let ui_view: *mut UIView = ui_view_ptr.cast();
                Retained::retain(ui_view).expect("UIView pointer should be valid")
            }
        }
        _ => {
            // Fallback: create a new UIView (won't be attached to window)
            eprintln!("[WARNING] Not a UIKit window handle, creating detached UIView");
            let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(390.0, 844.0));
            UIView::initWithFrame(mtm.alloc::<UIView>(), frame)
        }
    }
}
