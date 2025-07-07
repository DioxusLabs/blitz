use std::ops::{Deref, DerefMut};

use crate::DocumentHtmlParser;

use blitz_dom::{
    BaseDocument, DEFAULT_CSS, Document, DocumentConfig, EventDriver, NoopEventHandler,
};
use blitz_traits::events::UiEvent;

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
    fn handle_event(&mut self, event: UiEvent) {
        let mut driver = EventDriver::new(self.inner.mutate(), NoopEventHandler);
        driver.handle_ui_event(event);
    }

    fn id(&self) -> usize {
        self.inner.id()
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl HtmlDocument {
    /// Parse HTML (or XHTML) into an [`HtmlDocument`]
    pub fn from_html(html: &str, mut config: DocumentConfig) -> Self {
        if let Some(ss) = &mut config.ua_stylesheets {
            if !ss.iter().any(|s| s == DEFAULT_CSS) {
                ss.push(String::from(DEFAULT_CSS));
            }
        }
        let mut doc = BaseDocument::new(config);
        DocumentHtmlParser::parse_into_doc(&mut doc, html);
        HtmlDocument { inner: doc }
    }

    /// Convert the [`HtmlDocument`] into it's inner [`BaseDocument`]
    pub fn into_inner(self) -> BaseDocument {
        self.into()
    }
}
