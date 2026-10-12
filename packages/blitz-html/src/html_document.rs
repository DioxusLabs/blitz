use std::ops::{Deref, DerefMut};

use crate::DocumentHtmlParser;

use blitz_dom::{
    BaseDocument, DocGuard, DocGuardMut, Document, DocumentConfig, UserAgentStylesheet,
};

pub struct HtmlDocument {
    inner: BaseDocument,
}

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
    fn inner(&self) -> DocGuard<'_> {
        DocGuard::Ref(&self.inner)
    }

    fn inner_mut(&mut self) -> DocGuardMut<'_> {
        DocGuardMut::Ref(&mut self.inner)
    }
}

impl HtmlDocument {
    /// Parse HTML (or XHTML) into an [`HtmlDocument`].
    ///
    /// The content is sniffed to decide between HTML and XML parsing. Callers which
    /// know the document is XHTML from out-of-band information (a `Content-Type`
    /// header or an `.xht`/`.xhtml` file extension) should use
    /// [`from_xml`](Self::from_xml) instead, as the sniffing cannot detect all
    /// XHTML documents.
    pub fn from_html(html: &str, config: DocumentConfig) -> Self {
        Self::parse_with(html, config, DocumentHtmlParser::parse_into_mutator)
    }

    /// Parse XML (XHTML) into an [`HtmlDocument`]
    pub fn from_xml(xml: &str, config: DocumentConfig) -> Self {
        Self::parse_with(xml, config, DocumentHtmlParser::parse_xml_into_mutator)
    }

    fn parse_with(
        content: &str,
        mut config: DocumentConfig,
        parse: impl for<'a, 'd> Fn(&'a mut blitz_dom::DocumentMutator<'d>, &str),
    ) -> Self {
        let default_sheet = UserAgentStylesheet::default_sheet();
        let sheets = config.ua_stylesheets.get_or_insert_with(Vec::new);
        if !sheets.contains(&default_sheet) {
            sheets.insert(0, default_sheet);
        }
        let mut doc = BaseDocument::new(config);
        let mut mutr = doc.mutate();
        parse(&mut mutr, content);
        drop(mutr);
        HtmlDocument { inner: doc }
    }

    /// Convert the [`HtmlDocument`] into it's inner [`BaseDocument`]
    pub fn into_inner(self) -> BaseDocument {
        self.into()
    }
}
