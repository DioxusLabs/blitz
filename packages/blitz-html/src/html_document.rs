use std::sync::Arc;

use crate::DocumentHtmlParser;

use blitz_dom::{net::Resource, BaseDocument, FontContext, DEFAULT_CSS};
use blitz_traits::{
    navigation::NavigationProvider, net::SharedProvider, ColorScheme, Document, DomEvent, Viewport,
};

pub struct HtmlDocument {
    inner: BaseDocument,
}

// Implement DocumentLike and required traits for HtmlDocument

impl AsRef<BaseDocument> for HtmlDocument {
    fn as_ref(&self) -> &BaseDocument {
        &self.inner
    }
}
impl AsMut<BaseDocument> for HtmlDocument {
    fn as_mut(&mut self) -> &mut BaseDocument {
        &mut self.inner
    }
}
impl From<HtmlDocument> for BaseDocument {
    fn from(doc: HtmlDocument) -> BaseDocument {
        doc.inner
    }
}
impl Document for HtmlDocument {
    type Doc = BaseDocument;
    fn handle_event(&mut self, event: &mut DomEvent) {
        self.inner.as_mut().handle_event(event)
    }

    fn id(&self) -> usize {
        self.inner.id()
    }
}

impl HtmlDocument {
    pub fn from_html(
        html: &str,
        base_url: Option<String>,
        stylesheets: Vec<String>,
        net_provider: SharedProvider<Resource>,
        font_ctx: Option<FontContext>,
        navigation_provider: Arc<dyn NavigationProvider>,
    ) -> Self {
        // Spin up the virtualdom and include the default stylesheet
        let viewport = Viewport::new(0, 0, 1.0, ColorScheme::Light);
        let mut doc = match font_ctx {
            Some(font_ctx) => BaseDocument::with_font_ctx(viewport, font_ctx),
            None => BaseDocument::new(viewport),
        };

        // Set base url if configured
        if let Some(url) = &base_url {
            doc.set_base_url(url);
        }

        // Set the net provider
        doc.set_net_provider(net_provider.clone());

        // Set the navigation provider
        doc.set_navigation_provider(navigation_provider.clone());

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
