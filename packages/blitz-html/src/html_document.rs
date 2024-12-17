use crate::DocumentHtmlParser;

use blitz_dom::{
    events::RendererEvent, net::Resource, Document, DocumentLike, FontContext, DEFAULT_CSS,
};
use blitz_traits::{net::SharedProvider, ColorScheme, Viewport};

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
    fn handle_event(&mut self, event: RendererEvent) {
        self.inner.as_mut().handle_event(event);
    }
}

impl HtmlDocument {
    pub fn from_html(
        html: &str,
        base_url: Option<String>,
        stylesheets: Vec<String>,
        net_provider: SharedProvider<Resource>,
        font_ctx: Option<FontContext>,
    ) -> Self {
        // Spin up the virtualdom and include the default stylesheet
        let viewport = Viewport::new(0, 0, 1.0, ColorScheme::Light);
        let mut doc = match font_ctx {
            Some(font_ctx) => Document::with_font_ctx(viewport, font_ctx),
            None => Document::new(viewport),
        };

        // Set base url if configured
        if let Some(url) = &base_url {
            doc.set_base_url(url);
        }

        // Set the net provider
        doc.set_net_provider(net_provider.clone());

        // Include default and user-specified stylesheets
        doc.add_user_agent_stylesheet(DEFAULT_CSS);
        for ss in &stylesheets {
            doc.add_user_agent_stylesheet(ss);
        }

        // Parse HTML string into document
        DocumentHtmlParser::parse_into_doc(&mut doc, html, net_provider);

        HtmlDocument { inner: doc }
    }
}
