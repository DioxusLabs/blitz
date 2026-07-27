use crate::DocumentMutator;
use blitz_traits::node_id::NodeId;

pub trait HtmlParserProvider {
    fn parse_inner_html<'m, 'doc>(
        &self,
        mutr: &'m mut DocumentMutator<'doc>,
        element_id: NodeId,
        html: &str,
    );
}

pub struct DummyHtmlParserProvider;
impl HtmlParserProvider for DummyHtmlParserProvider {
    fn parse_inner_html<'m, 'doc>(
        &self,
        mutr: &'m mut DocumentMutator<'doc>,
        element_id: NodeId,
        html: &str,
    ) {
        let _ = mutr;
        let _ = element_id;
        let _ = html;
        // Do nothing for now
        //
        // TODO: do something:
        // - Print warning?
        // - Parse HTML as plain text?
    }
}
