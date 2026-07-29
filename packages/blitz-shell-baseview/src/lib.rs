#![cfg_attr(docsrs, feature(doc_cfg))]

//! A [baseview] shell for [Blitz](https://docs.rs/blitz), targeting embedded GUIs
//! such as audio plugin (CLAP/VST3) editors.
//!
//! Unlike [`blitz-shell`](https://docs.rs/blitz-shell) (which owns a winit event
//! loop), this crate renders a Blitz [`Document`] into a baseview window whose
//! event loop is driven by a host application:
//!
//!  - [`open_parented`] embeds the document as a child of a host-provided window
//!    (the typical audio-plugin scenario).
//!  - [`open_blocking`] opens a standalone top-level window and blocks until it
//!    is closed (useful for development and testing).
//!
//! Both functions take a *factory* closure that creates the document and renderer.
//! The closure runs on the window's GUI thread, which is what allows `Document`
//! (which is not `Send`) to be used.
//!
//! [baseview]: https://github.com/RustAudio/baseview

mod convert_events;
mod shell_provider;
mod view;

use std::cell::RefCell;

use anyrender::WindowRenderer;
use baseview::{Event, EventStatus, WindowContext, WindowHandler, WindowSize};
pub use baseview::{Window, WindowHandle, WindowOpenOptions, WindowScalePolicy, dpi};
use blitz_dom::Document;
use raw_window_handle::HasWindowHandle;

use crate::view::View;

/// A [`baseview::WindowHandler`] that renders a Blitz [`Document`].
///
/// baseview's handler methods take `&self` and are only ever invoked on the window's
/// GUI thread, so the mutable view state lives in a `RefCell`.
pub struct BlitzWindowHandler<Rend: WindowRenderer + 'static> {
    view: RefCell<View<Rend>>,
}

impl<Rend: WindowRenderer + 'static> BlitzWindowHandler<Rend> {
    /// Create a handler for the given document and renderer.
    ///
    /// Intended to be called from the `build` closure of [`baseview::Window::open_parented`]
    /// or [`baseview::Window::open_blocking`]. Prefer the [`open_parented`]/[`open_blocking`]
    /// convenience functions unless you need to wrap the handler yourself.
    pub fn new(doc: Box<dyn Document>, renderer: Rend, window: WindowContext) -> Self {
        Self {
            view: RefCell::new(View::init(doc, renderer, window)),
        }
    }
}

impl<Rend: WindowRenderer + 'static> WindowHandler for BlitzWindowHandler<Rend> {
    fn on_frame(&self) {
        self.view.borrow_mut().on_frame();
    }

    fn resized(&self, new_size: WindowSize) {
        self.view.borrow_mut().set_size(new_size);
    }

    fn on_event(&self, event: Event) -> EventStatus {
        self.view.borrow_mut().handle_event(event)
    }
}

/// Open a Blitz document as a child window of `parent`.
///
/// The returned [`WindowHandle`] closes the window when dropped, so it must be
/// kept alive (e.g. stored in the plugin editor struct) for as long as the
/// window should stay open.
pub fn open_parented<Rend, F>(
    parent: &impl HasWindowHandle,
    options: WindowOpenOptions,
    create: F,
) -> WindowHandle
where
    Rend: WindowRenderer + 'static,
    F: FnOnce() -> (Box<dyn Document>, Rend) + Send + 'static,
{
    Window::open_parented(parent, options, move |window| {
        let (doc, renderer) = create();
        BlitzWindowHandler::new(doc, renderer, window)
    })
}

/// Open a Blitz document in a standalone top-level window, blocking the calling
/// thread until the window is closed.
pub fn open_blocking<Rend, F>(options: WindowOpenOptions, create: F)
where
    Rend: WindowRenderer + 'static,
    F: FnOnce() -> (Box<dyn Document>, Rend) + Send + 'static,
{
    Window::open_blocking(options, move |window| {
        let (doc, renderer) = create();
        BlitzWindowHandler::new(doc, renderer, window)
    })
}
