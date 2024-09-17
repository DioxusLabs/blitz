use crate::events::RendererEvent;
use crate::{Document, DocumentHtmlParser, DocumentLike, Viewport};

use crate::util::Resource;
use crate::DEFAULT_CSS;
use blitz_traits::net::SharedCallback;

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
    pub fn from_html(
        html: &str,
        base_url: Option<String>,
        stylesheets: Vec<String>,
        shared_callback: SharedCallback<Resource>,
    ) -> Self {
        // Spin up the virtualdom and include the default stylesheet
        let viewport = Viewport::new(0, 0, 1.0);
        let mut dom = Document::new(viewport, Some(shared_callback));

        // Set base url if configured
        if let Some(url) = &base_url {
            dom.set_base_url(url);
        }

        // Include default and user-specified stylesheets
        dom.add_user_agent_stylesheet(DEFAULT_CSS);
        for ss in &stylesheets {
            dom.add_user_agent_stylesheet(ss);
        }

        // Parse HTML string into document
        DocumentHtmlParser::parse_into_doc(&mut dom, html);

        HtmlDocument { inner: dom }
    }
}
