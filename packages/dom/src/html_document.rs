use crate::events::RendererEvent;
use crate::{Document, DocumentHtmlParser, DocumentLike, Viewport};

use crate::DEFAULT_CSS;
use crate::util::{NetProvider, Resource};

pub struct HtmlDocument {
    inner: Document,
}

// Implement DocumentLike and required traits for HtmlDocument

impl AsRef<Document> for HtmlDocument {
    fn as_ref(&self) -> &Document {
        &self.inner
    }
}
impl AsMut<Document> for HtmlDocument {
    fn as_mut(&mut self) -> &mut Document {
        &mut self.inner
    }
}
impl From<HtmlDocument> for Document {
    fn from(doc: HtmlDocument) -> Document {
        doc.inner
    }
}
impl DocumentLike for HtmlDocument {
    fn handle_event(&mut self, event: RendererEvent) -> bool {
        self.inner.as_mut().handle_event(event)
    }
}

impl HtmlDocument {
    pub fn from_html<N: NetProvider<usize, Resource>>(html: &str, base_url: Option<String>, stylesheets: Vec<String>, net: N) -> Self {
        // Spin up the virtualdom and include the default stylesheet
        let viewport = Viewport::new(0, 0, 1.0);
        let mut dom = Document::new(viewport);

        // Set base url if configured
        if let Some(url) = &base_url {
            dom.set_base_url(url);
        }

        // Include default and user-specified stylesheets
        dom.add_stylesheet(DEFAULT_CSS);
        for ss in &stylesheets {
            dom.add_stylesheet(ss);
        }

        // Parse HTML string into document
        DocumentHtmlParser::parse_into_doc(&mut dom, net, html);

        HtmlDocument { inner: dom }
    }
}
