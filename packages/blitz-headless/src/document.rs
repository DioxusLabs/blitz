use std::sync::Arc;

use blitz_dom::{DocGuard, DocGuardMut, Document, DocumentConfig};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::net::NetProvider;
use blitz_traits::shell::{ColorScheme, Viewport};

use crate::render::{Screenshot, screenshot_document, screenshot_document_with_size};

/// Options controlling document construction for a [`HeadlessDocument`].
pub struct HeadlessOptions {
    /// Viewport width in physical pixels
    pub width: u32,
    /// Viewport height in physical pixels
    pub height: u32,
    pub scale: f32,
    pub color_scheme: ColorScheme,
    /// Base url which relative URLs are resolved against
    pub base_url: Option<String>,
    /// Net provider used to fetch sub-resources (stylesheets, images, fonts, etc)
    pub net_provider: Option<Arc<dyn NetProvider>>,
}

impl Default for HeadlessOptions {
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

impl HeadlessOptions {
    /// Convert into a [`DocumentConfig`] (with HTML parsing enabled)
    pub fn into_config(self) -> DocumentConfig {
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

/// A headless wrapper around a [`Document`] for resolving and rendering it
/// without a window.
pub struct HeadlessDocument<D: Document = HtmlDocument> {
    doc: D,
    net_provider: Option<Arc<dyn NetProvider>>,
}

impl HeadlessDocument<HtmlDocument> {
    /// Parse `html` into a document with default options (800x600 viewport, scale 1, light mode)
    pub fn from_html(html: &str) -> Self {
        Self::from_html_with(html, HeadlessOptions::default())
    }

    pub fn from_html_with(html: &str, options: HeadlessOptions) -> Self {
        let net_provider = options.net_provider.clone();
        let doc = HtmlDocument::from_html(html, options.into_config());
        Self { doc, net_provider }
    }
}

impl<D: Document> HeadlessDocument<D> {
    /// Wrap an already-constructed document, optionally with the [`NetProvider`] it
    /// fetches sub-resources through (used by
    /// [`resolve_until_network_idle`](Self::resolve_until_network_idle))
    pub fn wrap(doc: D, net_provider: Option<Arc<dyn NetProvider>>) -> Self {
        Self { doc, net_provider }
    }

    pub fn into_inner(self) -> D {
        self.doc
    }

    pub fn doc(&self) -> &D {
        &self.doc
    }

    pub fn doc_mut(&mut self) -> &mut D {
        &mut self.doc
    }

    /// Read access to the underlying [`blitz_dom::BaseDocument`]
    pub fn base(&self) -> DocGuard<'_> {
        self.doc.inner()
    }

    /// Write access to the underlying [`blitz_dom::BaseDocument`]
    pub fn base_mut(&mut self) -> DocGuardMut<'_> {
        self.doc.inner_mut()
    }

    /// Poll pending async work and resolve style/layout at animation time `time` (in seconds)
    pub fn resolve(&mut self, time: f64) {
        self.doc.poll(None);
        self.doc.inner_mut().resolve(time);
    }

    /// Repeatedly [`resolve`](Self::resolve) until the document's [`NetProvider`] reports
    /// no pending requests, so that sub-resources (stylesheets, images, fonts) and any
    /// resources they in turn trigger are loaded and reflected in style/layout.
    pub fn resolve_until_network_idle(&mut self) {
        loop {
            self.resolve(0.0);
            let pending = self
                .net_provider
                .as_ref()
                .map(|net| net.pending_requests())
                .unwrap_or(0);
            if pending == 0 {
                break;
            }
        }
    }

    /// The height of the document's root element in physical pixels (once laid out)
    pub fn content_height(&self) -> f32 {
        let doc = self.base();
        let scale = doc.get_viewport().scale();
        doc.root_element().final_layout().size.height * scale
    }

    /// Render the document to an RGBA screenshot at the viewport's physical size,
    /// on a white background, using the CPU renderer.
    ///
    /// Styles and layout must already be resolved (e.g. via
    /// [`resolve_until_network_idle`](Self::resolve_until_network_idle)).
    pub fn screenshot(&mut self) -> Screenshot {
        screenshot_document(&mut self.doc.inner_mut())
    }

    /// Render the document to an RGBA screenshot at the given physical size,
    /// on a white background, using the CPU renderer.
    ///
    /// Styles and layout must already be resolved (e.g. via
    /// [`resolve_until_network_idle`](Self::resolve_until_network_idle)).
    pub fn screenshot_with_size(&mut self, width: u32, height: u32) -> Screenshot {
        screenshot_document_with_size(&mut self.doc.inner_mut(), width, height)
    }
}
