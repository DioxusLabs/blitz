//! DOM and layout inspection helpers.

use std::fmt::Write as _;

use blitz_dom::{Document, Node, NodeData};
use blitz_traits::events::HitResult;
use blitz_traits::node_id::NodeId;

use crate::Harness;

/// An axis-aligned rectangle in page coordinates
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

impl<D: Document> Harness<D> {
    /// Find the first node matching `selector`.
    ///
    /// Panics if the selector fails to parse.
    pub fn query(&self, selector: &str) -> Option<NodeId> {
        self.base()
            .query_selector(selector)
            .expect("invalid selector")
    }

    /// Find the first node matching `selector`, panicking if there is no match
    pub fn node(&self, selector: &str) -> NodeId {
        self.query(selector)
            .unwrap_or_else(|| panic!("no node matching selector {selector:?}"))
    }

    /// All nodes matching `selector`
    pub fn query_all(&self, selector: &str) -> Vec<NodeId> {
        self.base()
            .query_selector_all(selector)
            .expect("invalid selector")
            .to_vec()
    }

    /// The border-box of the first element matching `selector`, in page coordinates
    pub fn layout_rect(&self, selector: &str) -> Rect {
        let node_id = self.node(selector);
        self.layout_rect_of(node_id)
    }

    /// The border-box of `node_id`, in page coordinates
    pub fn layout_rect_of(&self, node_id: NodeId) -> Rect {
        let doc = self.base();
        let node = doc.get_node(node_id).expect("node does not exist");
        let pos = node.absolute_position(0.0, 0.0);
        let layout = node.final_layout();
        Rect {
            x: pos.x,
            y: pos.y,
            width: layout.size.width,
            height: layout.size.height,
        }
    }

    /// The center of the border-box of the first element matching `selector`,
    /// in page coordinates
    pub fn center_of(&self, selector: &str) -> (f32, f32) {
        self.layout_rect(selector).center()
    }

    /// The text content of the first element matching `selector`
    pub fn text_content(&self, selector: &str) -> String {
        let node_id = self.node(selector);
        let doc = self.base();
        doc.get_node(node_id).unwrap().text_content()
    }

    /// The value of attribute `attr` on the first element matching `selector`
    pub fn attr(&self, selector: &str, attr: &str) -> Option<String> {
        let node_id = self.node(selector);
        let doc = self.base();
        let node = doc.get_node(node_id).unwrap();
        let attrs = node.attrs()?;
        attrs
            .iter()
            .find(|a| *a.name.local == *attr)
            .map(|a| a.value.clone())
    }

    /// Hit-test page coordinates `(x, y)`
    pub fn hit(&self, x: f32, y: f32) -> Option<HitResult> {
        self.base().hit(x, y)
    }

    /// The node currently targeted by a hit-test at `(x, y)`, panicking on miss
    pub fn hit_node(&self, x: f32, y: f32) -> NodeId {
        self.hit(x, y)
            .unwrap_or_else(|| panic!("hit test at ({x}, {y}) did not land on a node"))
            .node_id
    }

    /// The currently focussed node, if any
    pub fn focused(&self) -> Option<NodeId> {
        self.base().get_focussed_node_id()
    }

    /// The currently hovered node, if any
    pub fn hovered(&self) -> Option<NodeId> {
        self.base().get_hover_node_id()
    }

    /// A stable text serialization of the DOM tree, including box geometry.
    ///
    /// Suitable for snapshot-style assertions and debugging. One node per line:
    ///
    /// ```text
    /// <div #main .container> @ (0,0) 800x600
    ///   "Some text" @ (0,0) 100x20
    /// ```
    pub fn dom_string(&self) -> String {
        let mut out = String::new();
        let doc = self.base();
        let root = doc.root_node();
        write_node(&mut out, &doc, root, 0);
        out
    }
}

fn write_node(out: &mut String, doc: &blitz_dom::BaseDocument, node: &Node, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }

    // Only elements (and anonymous blocks) have their own layout box
    let geometry = match &node.data {
        NodeData::Element(_) | NodeData::AnonymousBlock(_) => {
            let pos = node.absolute_position(0.0, 0.0);
            let layout = node.final_layout();
            format!(
                " @ ({},{}) {}x{}",
                pos.x, pos.y, layout.size.width, layout.size.height
            )
        }
        _ => String::new(),
    };

    match &node.data {
        NodeData::Document(_) => writeln!(out, "#document").unwrap(),
        NodeData::AnonymousBlock(_) => writeln!(out, "#anonymous{geometry}").unwrap(),
        NodeData::Text(data) => {
            let content = data.content.trim();
            if content.is_empty() {
                return;
            }
            writeln!(out, "{:?}", truncate(content, 60)).unwrap();
        }
        NodeData::Comment { contents } => {
            writeln!(out, "<!-- {:?} -->", truncate(contents.trim(), 60)).unwrap();
        }
        NodeData::Element(data) => {
            write!(out, "<{}", data.name.local).unwrap();
            if let Some(id) = node.attr(blitz_dom::local_name!("id")) {
                write!(out, " #{id}").unwrap();
            }
            if let Some(class) = node.attr(blitz_dom::local_name!("class")) {
                for class in class.split_ascii_whitespace() {
                    write!(out, " .{class}").unwrap();
                }
            }
            writeln!(out, ">{geometry}").unwrap();
        }
    }

    for child_id in node.children.iter() {
        if let Some(child) = doc.get_node(*child_id) {
            write_node(out, doc, child, depth + 1);
        }
    }
}

fn truncate(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}
