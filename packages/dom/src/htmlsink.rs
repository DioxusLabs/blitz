use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;

use crate::node::{Attribute, ElementNodeData, Node, NodeData};
use crate::Document;
use html5ever::local_name;
use html5ever::{
    tendril::{StrTendril, TendrilSink},
    tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink},
    ExpandedName, QualName,
};
use style::Atom;

/// Convert an html5ever Attribute which uses tendril for its value to a blitz Attribute
/// which uses String.
fn html5ever_to_blitz_attr(attr: html5ever::Attribute) -> Attribute {
    Attribute {
        name: attr.name,
        value: attr.value.to_string(),
    }
}

pub struct DocumentHtmlParser<'a> {
    doc: &'a mut Document,

    style_nodes: Vec<usize>,

    /// Errors that occurred during parsing.
    pub errors: Vec<Cow<'static, str>>,

    /// The document's quirks mode.
    pub quirks_mode: QuirksMode,
}

impl<'a> DocumentHtmlParser<'a> {
    pub fn new<'b>(doc: &'b mut Document) -> DocumentHtmlParser<'b> {
        DocumentHtmlParser {
            doc,
            style_nodes: Vec::new(),
            errors: Vec::new(),
            quirks_mode: QuirksMode::NoQuirks,
        }
    }

    pub fn parse_into_doc<'d>(doc: &'d mut Document, html: &str) -> &'d mut Document {
        let sink = Self::new(doc);
        html5ever::parse_document(sink, Default::default())
            .from_utf8()
            .read_from(&mut html.as_bytes())
            .unwrap()
    }

    fn create_node(&mut self, node_data: NodeData) -> usize {
        self.doc.create_node(node_data)
    }

    fn create_text_node(&mut self, text: &str) -> usize {
        self.doc.create_text_node(text)
    }

    fn node(&self, id: usize) -> &Node {
        &self.doc.nodes[id]
    }

    fn node_mut(&mut self, id: usize) -> &mut Node {
        &mut self.doc.nodes[id]
    }

    fn try_append_text_to_text_node(&mut self, node_id: Option<usize>, text: &str) -> bool {
        let Some(node_id) = node_id else {
            return false;
        };
        let node = self.node_mut(node_id);

        match node.text_data_mut() {
            Some(data) => {
                data.content += text;
                true
            }
            None => false,
        }
    }

    fn last_child(&mut self, parent_id: usize) -> Option<usize> {
        self.node(parent_id).children.last().copied()
    }

    fn load_linked_stylesheet(&mut self, target_id: usize) {
        let node = self.node(target_id);

        let rel_attr = node.attr(local_name!("rel"));
        let href_attr = node.attr(local_name!("href"));

        if let (Some("stylesheet"), Some(href)) = (rel_attr, href_attr) {
            let url = self.doc.resolve_url(&href);
            match crate::util::fetch_string(url.as_str()) {
                Ok(css) => {
                    let css = html_escape::decode_html_entities(&css);
                    self.doc.add_stylesheet(&css);
                }
                Err(_) => eprintln!("Error fetching stylesheet {}", url),
            }
        }
    }

    fn load_image(&mut self, target_id: usize) {
        let node = self.node(target_id);
        if let Some(raw_src) = node.attr(local_name!("src")) {
            if raw_src.len() > 0 {
                let src = self.doc.resolve_url(&raw_src);

                // FIXME: Image fetching should not be a synchronous network request during parsing
                let image_result = crate::util::fetch_image(src.as_str());
                match image_result {
                    Ok(image) => {
                        self.node_mut(target_id).element_data_mut().unwrap().image =
                            Some(Arc::new(image));
                    }
                    Err(_) => {
                        eprintln!("Error fetching image {}", src);
                    }
                }
            }
        }
    }

    fn process_button_input(&mut self, target_id: usize) {
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
            let id = self.create_text_node(&value);
            self.append(&target_id, NodeOrText::AppendNode(id));
        }
    }
}

impl<'b> TreeSink for DocumentHtmlParser<'b> {
    type Output = &'b mut Document;

    // we use the ID of the nodes in the tree as the handle
    type Handle = usize;

    fn finish(self) -> Self::Output {
        // Add inline stylesheets (<style> elements)
        for id in &self.style_nodes {
            self.doc.process_style_element(*id);
        }

        // Compute child_idx fields.
        self.doc.flush_child_indexes(0, 0, 0);

        for error in self.errors {
            println!("ERROR: {}", error);
        }

        self.doc
    }

    fn parse_error(&mut self, msg: Cow<'static, str>) {
        self.errors.push(msg);
    }

    fn get_document(&mut self) -> Self::Handle {
        0
    }

    fn elem_name<'a>(&'a self, target: &'a Self::Handle) -> ExpandedName<'a> {
        self.node(*target)
            .element_data()
            .expect("TreeSink::elem_name called on a node which is not an element!")
            .name
            .expanded()
    }

    fn create_element(
        &mut self,
        name: QualName,
        attrs: Vec<html5ever::Attribute>,
        _flags: ElementFlags,
    ) -> Self::Handle {
        let attrs = attrs.into_iter().map(html5ever_to_blitz_attr).collect();
        let mut data = ElementNodeData::new(name.clone(), attrs);
        data.flush_style_attribute(&self.doc.guard);

        let id = self.create_node(NodeData::Element(data));
        let node = self.node(id);

        // Initialise style data
        *node.stylo_element_data.borrow_mut() = Some(Default::default());

        // If the node has an "id" attribute, store it in the ID map.
        if let Some(id_attr) = node.attr(local_name!("id")) {
            self.doc.nodes_to_id.insert(id_attr.to_string(), id);
        }

        // Custom post-processing by element tag name
        match name.local.as_ref() {
            "link" => self.load_linked_stylesheet(id),
            "img" => self.load_image(id),
            "input" => self.process_button_input(id),
            "style" => self.style_nodes.push(id),
            _ => {}
        }

        id
    }

    fn create_comment(&mut self, _text: StrTendril) -> Self::Handle {
        self.create_node(NodeData::Comment)
    }

    fn create_pi(&mut self, _target: StrTendril, _data: StrTendril) -> Self::Handle {
        // NOTE: html5ever does not call this method (only xml5ever does)
        unimplemented!()
    }

    fn append(&mut self, parent_id: &Self::Handle, child: NodeOrText<Self::Handle>) {
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
    fn append_before_sibling(
        &mut self,
        sibling_id: &Self::Handle,
        new_node: NodeOrText<Self::Handle>,
    ) {
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
                    let id = self.create_text_node(&text);
                    id
                }
            }
            NodeOrText::AppendNode(id) => id,
        };

        // TODO: Should remove from existing parent?
        assert_eq!(self.node_mut(new_child_id).parent, None);

        self.node_mut(new_child_id).parent = Some(parent_id);
        self.node_mut(parent_id)
            .children
            .insert(sibling_pos, new_child_id);
    }

    fn append_based_on_parent_node(
        &mut self,
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
        &mut self,
        _name: StrTendril,
        _public_id: StrTendril,
        _system_id: StrTendril,
    ) {
        // Ignore. We don't care about the DOCTYPE for now.
    }

    fn get_template_contents(&mut self, _target: &Self::Handle) -> Self::Handle {
        unimplemented!()
    }

    fn same_node(&self, x: &Self::Handle, y: &Self::Handle) -> bool {
        x == y
    }

    fn set_quirks_mode(&mut self, mode: QuirksMode) {
        self.quirks_mode = mode;
    }

    fn add_attrs_if_missing(&mut self, target: &Self::Handle, attrs: Vec<html5ever::Attribute>) {
        let element_data = self
            .node_mut(*target)
            .element_data_mut()
            .expect("Not an element");

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

    fn remove_from_parent(&mut self, target: &Self::Handle) {
        let node = self.node_mut(*target);
        let parent_id = node.parent.take().expect("Node has no parent");
        self.node_mut(parent_id)
            .children
            .retain(|child_id| child_id != target);
    }

    fn reparent_children(&mut self, node_id: &Self::Handle, new_parent_id: &Self::Handle) {
        // Take children array from old parent
        let node = self.node_mut(*node_id);
        let children = std::mem::replace(&mut node.children, Vec::new());

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
    use euclid::{Scale, Size2D};
    use style::media_queries::{Device, MediaType};

    let html = "<!DOCTYPE html><html><body><h1>hello world</h1></body></html>";
    let viewport_size = Size2D::new(800.0, 600.0);
    let device_pixel_ratio = Scale::new(1.0);

    let device = Device::new(
        MediaType::screen(),
        selectors::matching::QuirksMode::NoQuirks,
        viewport_size,
        device_pixel_ratio,
    );
    let mut doc = Document::new(device);
    let sink = DocumentHtmlParser::new(&mut doc);

    html5ever::parse_document(sink, Default::default())
        .from_utf8()
        .read_from(&mut html.as_bytes())
        .unwrap();

    doc.print_tree()

    // Now our tree should have some nodes in it
}
