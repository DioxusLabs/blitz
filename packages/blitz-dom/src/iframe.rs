//! Loading of `<iframe>` elements into sub-documents.

use std::sync::Arc;
use std::sync::mpsc::Sender;

use blitz_traits::navigation::{NavigationOptions, NavigationProvider};
use blitz_traits::net::{AbortController, AbortSignal, Url};
use blitz_traits::node_id::NodeId;
use blitz_traits::shell::ShellProvider;

use crate::document::DocumentEvent;
use crate::net::{DocumentSrcHandler, ResourceHandler, stamped_request};
use crate::{BaseDocument, DocumentConfig, local_name};

/// Maximum nesting depth of documents-within-documents. Iframes nested deeper
/// than this are not loaded (this guards against infinitely recursive
/// self-embedding pages).
pub(crate) const MAX_SUBDOCUMENT_DEPTH: usize = 10;

/// Per-iframe load state: the abort controller covering the current
/// "generation" of the iframe's content (the HTML fetch and, once loaded, the
/// sub-document's own sub-resource fetches), plus the id of the in-flight HTML
/// request (if any) so stale responses can be discarded.
pub(crate) struct IframeLoad {
    pub(crate) request_id: Option<usize>,
    pub(crate) abort_controller: AbortController,
}

/// A [`NavigationProvider`] given to iframe sub-documents. It routes
/// navigations (e.g. link clicks within the iframe) back to the parent
/// document, which reloads the iframe's sub-document with the new URL.
struct IframeNavigationProvider {
    node_id: NodeId,
    tx: Sender<DocumentEvent>,
    shell_provider: Arc<dyn ShellProvider>,
}

impl NavigationProvider for IframeNavigationProvider {
    fn navigate_to(&self, options: NavigationOptions) {
        let _ = self.tx.send(DocumentEvent::NavigateIframe {
            node_id: self.node_id,
            url: options.url,
        });
        self.shell_provider.request_redraw();
    }
}

impl BaseDocument {
    /// Build the [`DocumentConfig`] for an iframe sub-document, inheriting
    /// providers and settings from this (parent) document.
    fn iframe_document_config(
        &self,
        node_id: NodeId,
        base_url: Option<String>,
        abort_signal: AbortSignal,
    ) -> DocumentConfig {
        DocumentConfig {
            viewport: None,
            base_url,
            ua_stylesheets: None,
            net_provider: Some(self.net_provider.clone()),
            navigation_provider: Some(Arc::new(IframeNavigationProvider {
                node_id,
                tx: self.tx.clone(),
                shell_provider: self.shell_provider.clone(),
            })),
            shell_provider: Some(self.shell_provider.clone()),
            html_parser_provider: Some(self.html_parser_provider.clone()),
            font_ctx: Some(self.font_ctx.lock().unwrap().clone()),
            media_type: Some(self.media_type.clone()),
            style_threading: self.style_threading,
            incremental: Some(self.incremental_layout),
            abort_signal: Some(abort_signal),
            subdocument_depth: self.subdocument_depth + 1,
        }
    }

    /// Abort the current generation of the iframe's content (in-flight HTML
    /// fetch and/or the loaded sub-document's sub-resource fetches) and start
    /// a new generation.
    fn new_iframe_generation(&mut self, node_id: NodeId, request_id: Option<usize>) -> AbortSignal {
        if let Some(load) = self.iframe_loads.remove(&node_id) {
            load.abort_controller.abort();
        }
        let abort_controller = AbortController::default();
        let signal = abort_controller.signal.clone();
        self.iframe_loads.insert(
            node_id,
            IframeLoad {
                request_id,
                abort_controller,
            },
        );
        signal
    }

    /// Parse `html` and attach the resulting document as `node_id`'s
    /// sub-document.
    pub(crate) fn attach_iframe_document(
        &mut self,
        node_id: NodeId,
        html: &str,
        base_url: Option<String>,
        abort_signal: AbortSignal,
    ) {
        let config = self.iframe_document_config(node_id, base_url, abort_signal);
        let sub_doc = self
            .html_parser_provider
            .clone()
            .parse_document(html, config);
        self.set_sub_document(node_id, sub_doc);
        self.shell_provider.request_redraw();
    }

    /// Attach a sub-document parsed from the iframe's `srcdoc` attribute.
    /// Relative URLs within a `srcdoc` document resolve against the parent
    /// document's base URL.
    pub(crate) fn load_iframe_srcdoc(&mut self, node_id: NodeId, srcdoc: &str) {
        let signal = self.new_iframe_generation(node_id, None);
        let base_url = Some(self.url.to_string());
        self.attach_iframe_document(node_id, srcdoc, base_url, signal);
    }

    /// Start fetching HTML for an iframe from `url`. The parsed document is
    /// attached when the response arrives (via [`DocumentEvent::ResourceLoad`]).
    pub(crate) fn start_iframe_load(&mut self, node_id: NodeId, url: Url) {
        let handler = ResourceHandler::new(
            self.tx.clone(),
            self.id(),
            Some(node_id),
            self.shell_provider.clone(),
            DocumentSrcHandler,
        );
        let signal = self.new_iframe_generation(node_id, Some(handler.request_id()));
        self.net_provider.fetch(
            self.id(),
            stamped_request(url, Some(&signal)),
            Box::new(handler),
        );
    }

    /// Handle a navigation originating from within an iframe's sub-document
    /// (e.g. a link click) by loading the new URL into the iframe.
    pub(crate) fn navigate_iframe(&mut self, node_id: NodeId, url: Url) {
        let Some(node) = self.get_node(node_id) else {
            return;
        };
        if !node.flags.is_in_document() {
            return;
        }
        self.start_iframe_load(node_id, url);
    }

    /// Apply fetched iframe HTML, discarding stale responses (the iframe may
    /// have been removed or re-navigated since the request was issued).
    pub(crate) fn apply_iframe_html(
        &mut self,
        node_id: NodeId,
        request_id: usize,
        resolved_url: Option<String>,
        html: &str,
    ) {
        let is_current = self
            .iframe_loads
            .get(&node_id)
            .is_some_and(|load| load.request_id == Some(request_id));
        if !is_current {
            return;
        }
        let load = self.iframe_loads.get_mut(&node_id).unwrap();
        load.request_id = None;
        let signal = load.abort_controller.signal.clone();

        let node_is_iframe = self
            .get_node(node_id)
            .and_then(|node| node.element_data())
            .is_some_and(|el| el.name.local == local_name!("iframe"));
        if !node_is_iframe {
            return;
        }

        self.attach_iframe_document(node_id, html, resolved_url, signal);
    }
}
