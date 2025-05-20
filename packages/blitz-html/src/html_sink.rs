//! An implementation for Html5ever's sink trait, allowing us to parse HTML into a DOM.

use blitz_dom::net::{CssHandler, ImageHandler, Resource};
use blitz_dom::util::ImageType;
use std::borrow::Cow;
use std::cell::{Cell, Ref, RefCell, RefMut};
use std::collections::HashSet;

use blitz_dom::node::{Attribute, ElementNodeData, Node, NodeData};
use blitz_dom::BaseDocument;
use blitz_traits::net::{Request, SharedProvider};
use html5ever::{
    local_name,
    tendril::{StrTendril, TendrilSink},
    tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink},
    QualName,
};

/// Convert an html5ever Attribute which uses tendril for its value to a blitz Attribute
/// which uses String.
fn html5ever_to_blitz_attr(attr: html5ever::Attribute) -> Attribute {
    Attribute {
        name: attr.name,
        value: attr.value.to_string(),
    }
}

pub struct DocumentHtmlParser<'a> {
    doc_id: usize,
    doc: RefCell<&'a mut BaseDocument>,
    style_nodes: RefCell<Vec<usize>>,

    /// Errors that occurred during parsing.
    pub errors: RefCell<Vec<Cow<'static, str>>>,

    /// The document's quirks mode.
    pub quirks_mode: Cell<QuirksMode>,
    pub is_xml: bool,

    net_provider: SharedProvider<Resource>,
}

impl DocumentHtmlParser<'_> {
    pub fn new(
        doc: &mut BaseDocument,
        net_provider: SharedProvider<Resource>,
    ) -> DocumentHtmlParser {
        DocumentHtmlParser {
            doc_id: doc.id(),
            doc: RefCell::new(doc),
            style_nodes: RefCell::new(Vec::new()),
            errors: RefCell::new(Vec::new()),
            quirks_mode: Cell::new(QuirksMode::NoQuirks),
            net_provider,
            is_xml: false,
        }
    }

    pub fn parse_into_doc<'d>(
        doc: &'d mut BaseDocument,
        html: &str,
        net_provider: SharedProvider<Resource>,
    ) -> &'d mut BaseDocument {
        let mut sink = Self::new(doc, net_provider);
        if html.starts_with("<?xml")
            || html.starts_with("<!DOCTYPE") && {
                let first_line = html.lines().next().unwrap();
                first_line.contains("XHTML") || first_line.contains("xhtml")
            }
        {
            sink.is_xml = true;
            xml5ever::driver::parse_document(sink, Default::default())
                .from_utf8()
                .read_from(&mut html.as_bytes())
                .unwrap()
        } else {
            sink.is_xml = false;
            html5ever::parse_document(sink, Default::default())
                .from_utf8()
                .read_from(&mut html.as_bytes())
                .unwrap()
        }
    }

    #[track_caller]
    fn create_node(&self, node_data: NodeData) -> usize {
        self.doc.borrow_mut().create_node(node_data)
    }

    #[track_caller]
    fn create_text_node(&self, text: &str) -> usize {
        self.doc.borrow_mut().create_text_node(text)
    }

    #[track_caller]
    fn node(&self, id: usize) -> Ref<Node> {
        Ref::map(self.doc.borrow(), |doc| &doc.nodes[id])
    }

    #[track_caller]
    fn node_mut(&self, id: usize) -> RefMut<Node> {
        RefMut::map(self.doc.borrow_mut(), |doc| &mut doc.nodes[id])
    }

    fn try_append_text_to_text_node(&self, node_id: Option<usize>, text: &str) -> bool {
        let Some(node_id) = node_id else {
            return false;
        };
        let mut node = self.node_mut(node_id);

        match node.text_data_mut() {
            Some(data) => {
                data.content += text;
                true
            }
            None => false,
        }
    }

    fn last_child(&self, parent_id: usize) -> Option<usize> {
        self.node(parent_id).children.last().copied()
    }

    fn load_linked_stylesheet(&self, target_id: usize) {
        let node = self.node(target_id);

        let rel_attr = node.attr(local_name!("rel"));
        let href_attr = node.attr(local_name!("href"));

        if let (Some("stylesheet"), Some(href)) = (rel_attr, href_attr) {
            let url = self.doc.borrow().resolve_url(href);
            self.net_provider.fetch(
                self.doc_id,
                Request::get(url.clone()),
                Box::new(CssHandler {
                    node: target_id,
                    source_url: url,
                    guard: self.doc.borrow().guard.clone(),
                    provider: self.net_provider.clone(),
                }),
            );
        }
    }

    fn load_image(&self, target_id: usize) {
        let node = self.node(target_id);
        if let Some(raw_src) = node.attr(local_name!("src")) {
            if !raw_src.is_empty() {
                let src = self.doc.borrow().resolve_url(raw_src);
                self.net_provider.fetch(
                    self.doc.borrow().id(),
                    Request::get(src),
                    Box::new(ImageHandler::new(target_id, ImageType::Image)),
                );
            }
        }
    }

    fn process_button_input(&self, target_id: usize) {
        let node = self.node(target_id);
        let Some(data) = node.element_data() else {
            return;
        };

        let tagname = data.name.local.as_ref();
        let type_attr = data.attr(local_name!("type"));
        let value = data.attr(local_name!("value"));

        // Add content of "value" attribute as a text node child if:
        //   - Tag name is
        if let ("input", Some("button" | "submit" | "reset"), Some(value)) =
            (tagname, type_attr, value)
        {
            let value = value.to_string();
            drop(node);
            let id = self.create_text_node(&value);
            self.append(&target_id, NodeOrText::AppendNode(id));
        }
    }
}

impl<'b> TreeSink for DocumentHtmlParser<'b> {
    type Output = &'b mut BaseDocument;

    // we use the ID of the nodes in the tree as the handle
    type Handle = usize;

    type ElemName<'a>
        = Ref<'a, QualName>
    where
        Self: 'a;

    fn finish(self) -> Self::Output {
        let doc = self.doc.into_inner();

        // Add inline stylesheets (<style> elements)
        for id in self.style_nodes.borrow().iter() {
            doc.process_style_element(*id);
        }

        for error in self.errors.borrow().iter() {
            println!("ERROR: {}", error);
        }

        doc
    }

    fn parse_error(&self, msg: Cow<'static, str>) {
        self.errors.borrow_mut().push(msg);
    }

    fn get_document(&self) -> Self::Handle {
        0
    }

    fn elem_name<'a>(&'a self, target: &'a Self::Handle) -> Self::ElemName<'a> {
        Ref::map(self.doc.borrow(), |doc| {
            &doc.nodes[*target]
                .element_data()
                .expect("TreeSink::elem_name called on a node which is not an element!")
                .name
        })
    }

    fn create_element(
        &self,
        name: QualName,
        attrs: Vec<html5ever::Attribute>,
        _flags: ElementFlags,
    ) -> Self::Handle {
        let attrs = attrs.into_iter().map(html5ever_to_blitz_attr).collect();
        let mut data = ElementNodeData::new(name.clone(), attrs);
        data.flush_style_attribute(&self.doc.borrow().guard);

        let id = self.create_node(NodeData::Element(data));
        let node = self.node(id);

        // Initialise style data
        *node.stylo_element_data.borrow_mut() = Some(Default::default());

        let id_attr = node.attr(local_name!("id")).map(|id| id.to_string());
        drop(node);

        // If the node has an "id" attribute, store it in the ID map.
        if let Some(id_attr) = id_attr {
            self.doc.borrow_mut().nodes_to_id.insert(id_attr, id);
        }

        // Custom post-processing by element tag name
        match name.local.as_ref() {
            "link" => self.load_linked_stylesheet(id),
            "img" => self.load_image(id),
            "input" => self.process_button_input(id),
            "style" => self.style_nodes.borrow_mut().push(id),
            _ => {}
        }

        id
    }

    fn create_comment(&self, _text: StrTendril) -> Self::Handle {
        self.create_node(NodeData::Comment)
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> Self::Handle {
        self.create_node(NodeData::Comment)
    }

    fn append(&self, parent_id: &Self::Handle, child: NodeOrText<Self::Handle>) {
        match child {
            NodeOrText::AppendNode(child_id) => {
                self.node_mut(*parent_id).children.push(child_id);
                self.node_mut(child_id).parent = Some(*parent_id);
            }
            NodeOrText::AppendText(text) => {
                let last_child_id = self.last_child(*parent_id);
                let has_appended = self.try_append_text_to_text_node(last_child_id, &text);
                if !has_appended {
                    let id = self.create_text_node(&text);
                    self.append(parent_id, NodeOrText::AppendNode(id));
                }
            }
        }
    }

    // Note: The tree builder promises we won't have a text node after the insertion point.
    // https://github.com/servo/html5ever/blob/main/rcdom/lib.rs#L338
    fn append_before_sibling(&self, sibling_id: &Self::Handle, new_node: NodeOrText<Self::Handle>) {
        let sibling = self.node(*sibling_id);
        let parent_id = sibling.parent.expect("Sibling has not parent");
        let parent = self.node(parent_id);
        let sibling_pos = parent
            .children
            .iter()
            .position(|cid| cid == sibling_id)
            .expect("Sibling is not a child of parent");

        // If node to append is a text node, first attempt to
        let new_child_id = match new_node {
            NodeOrText::AppendText(text) => {
                let previous_sibling_id = match sibling_pos {
                    0 => None,
                    other => Some(parent.children[other - 1]),
                };
                let has_appended = self.try_append_text_to_text_node(previous_sibling_id, &text);
                if has_appended {
                    return;
                } else {
                    self.create_text_node(&text)
                }
            }
            NodeOrText::AppendNode(id) => id,
        };

        // TODO: Should remove from existing parent?
        assert_eq!(self.node(new_child_id).parent, None);

        drop(parent);
        drop(sibling);

        self.node_mut(new_child_id).parent = Some(parent_id);
        self.node_mut(parent_id)
            .children
            .insert(sibling_pos, new_child_id);
    }

    fn append_based_on_parent_node(
        &self,
        element: &Self::Handle,
        prev_element: &Self::Handle,
        child: NodeOrText<Self::Handle>,
    ) {
        let has_parent = self.node(*element).parent.is_some();
        if has_parent {
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
        let mut node = self.node_mut(*target);
        let element_data = node.element_data_mut().expect("Not an element");

        let existing_names = element_data
            .attrs
            .iter()
            .map(|e| e.name.clone())
            .collect::<HashSet<_>>();

        element_data.attrs.extend(
            attrs
                .into_iter()
                .map(html5ever_to_blitz_attr)
                .filter(|attr| !existing_names.contains(&attr.name)),
        );
    }

    fn remove_from_parent(&self, target: &Self::Handle) {
        let parent_id = self
            .node_mut(*target)
            .parent
            .take()
            .expect("Node has no parent");
        self.node_mut(parent_id)
            .children
            .retain(|child_id| child_id != target);
    }

    fn reparent_children(&self, node_id: &Self::Handle, new_parent_id: &Self::Handle) {
        // Take children array from old parent
        let children = std::mem::take(&mut self.node_mut(*node_id).children);

        // Update parent reference of children
        for child_id in children.iter() {
            self.node_mut(*child_id).parent = Some(*new_parent_id);
        }

        // Add children to new parent
        self.node_mut(*new_parent_id).children.extend(&children);
    }
}

#[test]
fn parses_some_html() {
    use blitz_traits::{net::DummyNetProvider, ColorScheme, Viewport};
    use std::sync::Arc;

    let html = "<!DOCTYPE html><html><body><h1>hello world</h1></body></html>";
    let viewport = Viewport::new(800, 600, 1.0, ColorScheme::Light);
    let mut doc = BaseDocument::new(viewport);
    let sink = DocumentHtmlParser::new(&mut doc, Arc::new(DummyNetProvider::default()));

    html5ever::parse_document(sink, Default::default())
        .from_utf8()
        .read_from(&mut html.as_bytes())
        .unwrap();

    doc.print_tree()

    // Now our tree should have some nodes in it
}
