//! An implementation for Html5ever's sink trait, allowing us to parse HTML into a DOM.

use html5ever::ParseOpts;
use html5ever::tokenizer::TokenizerOpts;
use html5ever::tree_builder::TreeBuilderOpts;
use std::borrow::Cow;
use std::cell::{Cell, Ref, RefCell, RefMut};

use blitz_dom::node::Attribute;
use blitz_dom::{DocumentMutator, HtmlParserProvider};
use html5ever::{
    QualName,
    tendril::{StrTendril, TendrilSink},
    tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink},
};

/// Convert an html5ever Attribute which uses tendril for its value to a blitz Attribute
/// which uses String.
fn html5ever_to_blitz_attr(attr: html5ever::Attribute) -> Attribute {
    Attribute {
        name: attr.name,
        value: attr.value.to_string(),
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub struct HtmlProvider;

impl HtmlParserProvider for HtmlProvider {
    fn parse_inner_html<'m2, 'doc2>(
        &self,
        mutr: &'m2 mut DocumentMutator<'doc2>,
        element_id: usize,
        html: &str,
    ) {
        DocumentHtmlParser::parse_inner_html_into_mutator(mutr, element_id, html);
    }
}

pub struct DocumentHtmlParser<'m, 'doc> {
    document_mutator: RefCell<&'m mut DocumentMutator<'doc>>,

    /// Errors that occurred during parsing.
    pub errors: RefCell<Vec<Cow<'static, str>>>,

    /// The document's quirks mode.
    pub quirks_mode: Cell<QuirksMode>,
    pub is_xml: bool,
}

impl<'m, 'doc> DocumentHtmlParser<'m, 'doc> {
    #[track_caller]
    /// Get a mutable borrow of the DocumentMutator
    fn mutr(&self) -> RefMut<'_, &'m mut DocumentMutator<'doc>> {
        self.document_mutator.borrow_mut()
    }
}

impl<'m, 'doc> DocumentHtmlParser<'m, 'doc> {
    pub fn new(mutr: &'m mut DocumentMutator<'doc>) -> DocumentHtmlParser<'m, 'doc> {
        DocumentHtmlParser {
            document_mutator: RefCell::new(mutr),
            errors: RefCell::new(Vec::new()),
            quirks_mode: Cell::new(QuirksMode::NoQuirks),
            is_xml: false,
        }
    }

    pub fn parse_into_mutator<'a, 'd>(mutr: &'a mut DocumentMutator<'d>, html: &str) {
        let mut sink = DocumentHtmlParser::new(mutr);

        let is_xhtml_doc = html.starts_with("<?xml")
            || html.starts_with("<!DOCTYPE") && {
                let first_line = html.lines().next().unwrap();
                first_line.contains("XHTML") || first_line.contains("xhtml")
            };

        if is_xhtml_doc {
            // Parse as XHTML
            sink.is_xml = true;
            xml5ever::driver::parse_document(sink, Default::default())
                .from_utf8()
                .read_from(&mut html.as_bytes())
                .unwrap();
        } else {
            // Parse as HTML
            sink.is_xml = false;
            let opts = ParseOpts {
                tokenizer: TokenizerOpts::default(),
                tree_builder: TreeBuilderOpts {
                    exact_errors: false,
                    scripting_enabled: false, // Enables parsing of <noscript> tags
                    iframe_srcdoc: false,
                    drop_doctype: true,
                    quirks_mode: QuirksMode::NoQuirks,
                },
            };
            html5ever::parse_document(sink, opts)
                .from_utf8()
                .read_from(&mut html.as_bytes())
                .unwrap();
        }
    }

    pub fn parse_inner_html_into_mutator<'a, 'd>(
        mutr: &'a mut DocumentMutator<'d>,
        element_id: usize,
        html: &str,
    ) {
        let sink = DocumentHtmlParser::new(mutr);

        let opts = ParseOpts {
            tokenizer: TokenizerOpts::default(),
            tree_builder: TreeBuilderOpts {
                exact_errors: false,
                scripting_enabled: false, // Enables parsing of <noscript> tags
                iframe_srcdoc: false,
                drop_doctype: true,
                quirks_mode: QuirksMode::NoQuirks,
            },
        };
        html5ever::driver::parse_fragment_for_element(sink, opts, element_id, false, None)
            .from_utf8()
            .read_from(&mut html.as_bytes())
            .unwrap();

        // html5ever creates a new fragment root node under the document node and parses the nodes into that fragment root.
        // So here we move the children of the fragment root to element_id and then remove the fragment root
        let fragment_root_id = mutr.last_child_id(0).unwrap();
        let child_ids = mutr.child_ids(fragment_root_id);
        mutr.append_children(element_id, &child_ids);
        mutr.remove_node(fragment_root_id);
    }
}

impl<'m, 'doc> TreeSink for DocumentHtmlParser<'m, 'doc> {
    type Output = ();

    // we use the ID of the nodes in the tree as the handle
    type Handle = usize;

    type ElemName<'a>
        = Ref<'a, QualName>
    where
        Self: 'a;

    fn finish(self) -> Self::Output {
        #[cfg(feature = "tracing")]
        for error in self.errors.borrow().iter() {
            tracing::error!("{error}");
        }
    }

    fn parse_error(&self, msg: Cow<'static, str>) {
        self.errors.borrow_mut().push(msg);
    }

    fn get_document(&self) -> Self::Handle {
        0
    }

    fn elem_name<'a>(&'a self, target: &'a Self::Handle) -> Self::ElemName<'a> {
        Ref::map(self.document_mutator.borrow(), |docm| {
            docm.element_name(*target)
                .expect("TreeSink::elem_name called on a node which is not an element!")
        })
    }

    fn create_element(
        &self,
        name: QualName,
        attrs: Vec<html5ever::Attribute>,
        _flags: ElementFlags,
    ) -> Self::Handle {
        let attrs = attrs.into_iter().map(html5ever_to_blitz_attr).collect();
        self.mutr().create_element(name, attrs)
    }

    fn create_comment(&self, _text: StrTendril) -> Self::Handle {
        self.mutr().create_comment_node()
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> Self::Handle {
        self.mutr().create_comment_node()
    }

    fn append(&self, parent_id: &Self::Handle, child: NodeOrText<Self::Handle>) {
        match child {
            NodeOrText::AppendNode(id) => self.mutr().append_children(*parent_id, &[id]),
            // If content to append is text, first attempt to append it to the last child of parent.
            // Else create a new text node and append it to the parent
            NodeOrText::AppendText(text) => {
                let last_child_id = self.mutr().last_child_id(*parent_id);
                let has_appended = if let Some(id) = last_child_id {
                    self.mutr().append_text_to_node(id, &text).is_ok()
                } else {
                    false
                };
                if !has_appended {
                    let new_child_id = self.mutr().create_text_node(&text);
                    self.mutr().append_children(*parent_id, &[new_child_id]);
                }
            }
        }
    }

    // Note: The tree builder promises we won't have a text node after the insertion point.
    // https://github.com/servo/html5ever/blob/main/rcdom/lib.rs#L338
    fn append_before_sibling(&self, sibling_id: &Self::Handle, new_node: NodeOrText<Self::Handle>) {
        match new_node {
            NodeOrText::AppendNode(id) => self.mutr().insert_nodes_before(*sibling_id, &[id]),
            // If content to append is text, first attempt to append it to the node before sibling_node
            // Else create a new text node and insert it before sibling_node
            NodeOrText::AppendText(text) => {
                let previous_sibling_id = self.mutr().previous_sibling_id(*sibling_id);
                let has_appended = if let Some(id) = previous_sibling_id {
                    self.mutr().append_text_to_node(id, &text).is_ok()
                } else {
                    false
                };
                if !has_appended {
                    let new_child_id = self.mutr().create_text_node(&text);
                    self.mutr()
                        .insert_nodes_before(*sibling_id, &[new_child_id]);
                }
            }
        };
    }

    fn append_based_on_parent_node(
        &self,
        element: &Self::Handle,
        prev_element: &Self::Handle,
        child: NodeOrText<Self::Handle>,
    ) {
        if self.mutr().node_has_parent(*element) {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(
        &self,
        _name: StrTendril,
        _public_id: StrTendril,
        _system_id: StrTendril,
    ) {
        // Ignore. We don't care about the DOCTYPE for now.
    }

    fn get_template_contents(&self, target: &Self::Handle) -> Self::Handle {
        // TODO: implement templates properly. This should allow to function like regular elements.
        *target
    }

    fn same_node(&self, x: &Self::Handle, y: &Self::Handle) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, mode: QuirksMode) {
        self.quirks_mode.set(mode);
    }

    fn add_attrs_if_missing(&self, target: &Self::Handle, attrs: Vec<html5ever::Attribute>) {
        let attrs = attrs.into_iter().map(html5ever_to_blitz_attr).collect();
        self.mutr().add_attrs_if_missing(*target, attrs);
    }

    fn remove_from_parent(&self, target: &Self::Handle) {
        self.mutr().remove_node(*target);
    }

    fn reparent_children(&self, old_parent_id: &Self::Handle, new_parent_id: &Self::Handle) {
        self.mutr()
            .reparent_children(*old_parent_id, *new_parent_id);
    }

    fn clone_subtree(&self, target: &Self::Handle) -> Self::Handle {
        self.mutr().deep_clone_node(*target)
    }
}

#[test]
fn parses_some_html() {
    use blitz_dom::{BaseDocument, DocumentConfig};

    let html = "<!DOCTYPE html><html><body><h1>hello world</h1></body></html>";
    let mut doc = BaseDocument::new(DocumentConfig::default());
    let mut mutr = doc.mutate();
    let sink = DocumentHtmlParser::new(&mut mutr);

    html5ever::parse_document(sink, Default::default())
        .from_utf8()
        .read_from(&mut html.as_bytes())
        .unwrap();

    drop(mutr);
    doc.print_tree()

    // Now our tree should have some nodes in it
}
