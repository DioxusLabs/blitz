use std::sync::Arc;

use blitz_dom::{DocGuard, DocGuardMut, Document, DocumentConfig};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::events::UiEvent;
use blitz_traits::net::NetProvider;
use blitz_traits::shell::{ColorScheme, Viewport};
use dioxus_core::{Element, VirtualDom};
use dioxus_native_dom::DioxusDocument;

/// Options controlling document construction for a [`Harness`].
pub struct HarnessOptions {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub color_scheme: ColorScheme,
    /// Base url which relative URLs are resolved against
    pub base_url: Option<String>,
    /// Net provider used to fetch sub-resources (stylesheets, images, fonts, etc)
    pub net_provider: Option<Arc<dyn NetProvider>>,
}

impl Default for HarnessOptions {
    fn default() -> Self {
        Self {
            width: 800,
            height: 600,
            scale: 1.0,
            color_scheme: ColorScheme::Light,
            base_url: None,
            net_provider: None,
        }
    }
}

impl HarnessOptions {
    fn into_config(self) -> DocumentConfig {
        DocumentConfig {
            viewport: Some(Viewport::new(
                self.width,
                self.height,
                self.scale,
                self.color_scheme,
            )),
            base_url: self.base_url,
            net_provider: self.net_provider,
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        }
    }
}

/// A headless wrapper around a [`Document`] for driving it programmatically in tests.
pub struct Harness<D: Document = HtmlDocument> {
    pub doc: D,
    time: f64,
}

impl Harness<HtmlDocument> {
    /// Parse `html` into a document with default options (800x600 viewport, scale 1, light mode)
    pub fn from_html(html: &str) -> Self {
        Self::from_html_with(html, HarnessOptions::default())
    }

    pub fn from_html_with(html: &str, options: HarnessOptions) -> Self {
        let doc = HtmlDocument::from_html(html, options.into_config());
        let mut harness = Self::wrap(doc);
        harness.pump();
        harness
    }
}

impl Harness<DioxusDocument> {
    /// Build a document from a Dioxus root component with default options
    pub fn from_component(app: fn() -> Element) -> Self {
        Self::from_vdom(VirtualDom::new(app), HarnessOptions::default())
    }

    pub fn from_vdom(vdom: VirtualDom, options: HarnessOptions) -> Self {
        let mut doc = DioxusDocument::new(vdom, options.into_config());
        doc.initial_build();
        let mut harness = Self::wrap(doc);
        harness.pump();
        harness
    }
}

impl<D: Document> Harness<D> {
    /// Wrap an already-constructed document. Does not [`pump`](Self::pump).
    pub fn wrap(doc: D) -> Self {
        Self { doc, time: 0.0 }
    }

    pub fn into_inner(self) -> D {
        self.doc
    }

    /// Read access to the underlying [`blitz_dom::BaseDocument`]
    pub fn base(&self) -> DocGuard<'_> {
        self.doc.inner()
    }

    /// Write access to the underlying [`blitz_dom::BaseDocument`]
    pub fn base_mut(&mut self) -> DocGuardMut<'_> {
        self.doc.inner_mut()
    }

    /// The current time (in seconds) of the harness's animation clock
    pub fn time(&self) -> f64 {
        self.time
    }

    /// Poll pending async work (e.g. the Dioxus VirtualDom) and resolve style/layout
    pub fn pump(&mut self) {
        self.doc.poll(None);
        self.doc.inner_mut().resolve(self.time);
    }

    /// Advance the animation clock by `dt_seconds` and [`pump`](Self::pump)
    pub fn tick(&mut self, dt_seconds: f64) {
        self.time += dt_seconds;
        self.pump();
    }

    /// Dispatch a raw [`UiEvent`] through the document's event pipeline.
    ///
    /// Unlike the higher-level input helpers this does not [`pump`](Self::pump) afterwards.
    pub fn dispatch(&mut self, event: UiEvent) {
        self.doc.handle_ui_event(event);
    }

    /// Dispatch raw [`UiEvent`]s, recording the name of every DOM event dispatched
    /// to application code.
    ///
    /// The events are driven directly against the underlying [`blitz_dom::BaseDocument`],
    /// so document-specific event handling (e.g. forwarding to a Dioxus VirtualDom) is
    /// bypassed. Does not [`pump`](Self::pump).
    pub fn dispatch_recorded(&mut self, events: impl IntoIterator<Item = UiEvent>) -> Vec<String> {
        use blitz_dom::{EventDriver, EventHandler};
        use blitz_traits::events::{DomEvent, EventState};
        use std::cell::RefCell;
        use std::rc::Rc;

        #[derive(Clone, Default)]
        struct RecordingHandler {
            events: Rc<RefCell<Vec<String>>>,
        }

        impl EventHandler for RecordingHandler {
            fn handle_event(
                &mut self,
                _chain: &[blitz_traits::node_id::NodeId],
                event: &mut DomEvent,
                _doc: &mut dyn Document,
                _event_state: &mut EventState,
            ) {
                self.events.borrow_mut().push(event.name().to_string());
            }
        }

        let handler = RecordingHandler::default();
        let recorded = handler.events.clone();
        let mut doc = self.doc.inner_mut();
        let mut driver = EventDriver::new(&mut *doc, handler);
        for event in events {
            driver.handle_ui_event(event);
        }
        drop(doc);
        recorded.borrow().clone()
    }

    /// Resize the viewport, keeping the current scale and color scheme
    pub fn set_viewport_size(&mut self, width: u32, height: u32) {
        let mut doc = self.doc.inner_mut();
        let viewport = doc.get_viewport();
        doc.set_viewport(Viewport::new(
            width,
            height,
            viewport.hidpi_scale,
            viewport.color_scheme,
        ));
        drop(doc);
        self.pump();
    }
}
