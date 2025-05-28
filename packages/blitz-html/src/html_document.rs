use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

use crate::DocumentHtmlParser;

use blitz_dom::{
    BaseDocument, DEFAULT_CSS, Document, EventDriver, FontContext, NoopEventHandler, net::Resource,
};
use blitz_traits::{
    ColorScheme, DomEvent, Viewport, navigation::NavigationProvider, net::SharedProvider,
};

pub struct HtmlDocument {
    inner: BaseDocument,
}

// Implement DocumentLike and required traits for HtmlDocument
impl Deref for HtmlDocument {
    type Target = BaseDocument;
    fn deref(&self) -> &BaseDocument {
        &self.inner
    }
}
impl DerefMut for HtmlDocument {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
impl From<HtmlDocument> for BaseDocument {
    fn from(doc: HtmlDocument) -> BaseDocument {
        doc.inner
    }
}
impl Document for HtmlDocument {
    fn handle_event(&mut self, event: DomEvent) {
        let mut driver = EventDriver::new(self.inner.mutate(), NoopEventHandler);
        driver.handle_event(event);
    }

    fn id(&self) -> usize {
        self.inner.id()
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
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
        DocumentHtmlParser::parse_into_doc(&mut doc, html);

        HtmlDocument { inner: doc }
    }
}
