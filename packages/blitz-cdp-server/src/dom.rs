use std::collections::HashSet;

use blitz_dom::BaseDocument;
use blitz_dom::node::{Node, NodeData};
use blitz_traits::node_id::NodeId;
use serde_json::json;

use crate::JsonValue;
use crate::session::cdp_node_id;

const ELEMENT_NODE: u32 = 1;
const TEXT_NODE: u32 = 3;
const COMMENT_NODE: u32 = 8;
const DOCUMENT_NODE: u32 = 9;

/// The DOM children of a node (excluding anonymous boxes, which live in
/// the layout tree rather than the DOM tree, and whitespace-only text
/// nodes, which are just noise in the inspector)
pub(crate) fn dom_children<'doc>(doc: &'doc BaseDocument, node: &Node) -> Vec<&'doc Node> {
    node.children
        .iter()
        .filter_map(|child_id| doc.get_node(*child_id))
        .filter(|child| !child.is_anonymous() && !child.is_whitespace_node())
        .collect()
}

/// Serialize a node to its CDP `DOM.Node` form. `depth` controls how many
/// levels of children are included: 0 for none, a negative value for the
/// entire subtree. Nodes whose children are included are recorded in
/// `children_sent`, so that later `DOM.setChildNodes` events can avoid
/// resending children the frontend already knows about (which would replace
/// its node objects and break e.g. revealing a picked node in the tree).
pub(crate) fn node_json(
    doc: &BaseDocument,
    node_id: NodeId,
    depth: i64,
    children_sent: &mut HashSet<NodeId>,
) -> Option<JsonValue> {
    let node = doc.get_node(node_id)?;

    let (node_type, node_name, node_value) = match &node.data {
        NodeData::Document(_) => (DOCUMENT_NODE, "#document".to_string(), String::new()),
        NodeData::Element(el) | NodeData::AnonymousBlock(el) => {
            (ELEMENT_NODE, el.name.local.to_uppercase(), String::new())
        }
        NodeData::Text(_) => (TEXT_NODE, "#text".to_string(), node.text_content()),
        NodeData::Comment => (COMMENT_NODE, "#comment".to_string(), String::new()),
    };

    let local_name = match &node.data {
        NodeData::Element(el) | NodeData::AnonymousBlock(el) => el.name.local.to_string(),
        _ => String::new(),
    };

    let children = dom_children(doc, node);
    let child_count = children.len();

    let mut form = json!({
        "nodeId": cdp_node_id(node_id),
        "backendNodeId": cdp_node_id(node_id),
        "nodeType": node_type,
        "nodeName": node_name,
        "localName": local_name,
        "nodeValue": node_value,
        "childNodeCount": child_count,
    });

    if let Some(parent_id) = node.parent {
        form["parentId"] = json!(cdp_node_id(parent_id));
    }

    if let Some(attrs) = node.attrs() {
        let flat: Vec<String> = attrs
            .iter()
            .flat_map(|attr| [attr.name.local.to_string(), attr.value.to_string()])
            .collect();
        form["attributes"] = json!(flat);
    }

    if matches!(&node.data, NodeData::Document(_)) {
        form["documentURL"] = json!(doc.url().to_string());
        form["baseURL"] = json!(doc.url().to_string());
    }

    // Include children when requested by depth. Like Chrome, also inline a
    // single text child regardless of depth so the markup view can render
    // `<tag>text</tag>` on one line.
    let single_text_child = child_count == 1 && children[0].is_text_node();
    if depth != 0 || single_text_child {
        let child_forms: Vec<JsonValue> = children
            .iter()
            .filter_map(|child| {
                node_json(doc, child.id, depth.saturating_sub(1).max(0), children_sent)
            })
            .collect();
        form["children"] = json!(child_forms);
        children_sent.insert(node_id);
    }

    Some(form)
}

/// A CDP `Quad`: an array of 8 numbers `[x1, y1, x2, y2, x3, y3, x4, y4]`
/// describing the corners of a rectangle in clockwise order
fn quad(x: f64, y: f64, width: f64, height: f64) -> JsonValue {
    json!([x, y, x + width, y, x + width, y + height, x, y + height])
}

/// Build the `DOM.getBoxModel` response for a node
pub(crate) fn box_model_json(doc: &BaseDocument, node_id: NodeId) -> Option<JsonValue> {
    let node = doc.get_node(node_id)?;
    node.element_data()?;

    // The border-box rect, viewport-relative. Non-atomic inline elements
    // have no layout box of their own: this is the bounding rect of their
    // per-line-box fragments.
    let rect = doc.get_client_bounding_rect(node_id)?;

    // Non-atomic inline elements report zero box insets
    let is_inline_fragment = doc.inline_fragment_rects(node_id).is_some();
    let layout = node.final_layout();
    let (border, padding, margin) = if is_inline_fragment {
        Default::default()
    } else {
        (layout.border, layout.padding, layout.margin)
    };

    let border_x = rect.x;
    let border_y = rect.y;
    let border_w = rect.width;
    let border_h = rect.height;

    let padding_x = border_x + border.left as f64;
    let padding_y = border_y + border.top as f64;
    let padding_w = border_w - (border.left + border.right) as f64;
    let padding_h = border_h - (border.top + border.bottom) as f64;

    let content_x = padding_x + padding.left as f64;
    let content_y = padding_y + padding.top as f64;
    let content_w = padding_w - (padding.left + padding.right) as f64;
    let content_h = padding_h - (padding.top + padding.bottom) as f64;

    let margin_x = border_x - margin.left as f64;
    let margin_y = border_y - margin.top as f64;
    let margin_w = border_w + (margin.left + margin.right) as f64;
    let margin_h = border_h + (margin.top + margin.bottom) as f64;

    Some(json!({
        "content": quad(content_x, content_y, content_w, content_h),
        "padding": quad(padding_x, padding_y, padding_w, padding_h),
        "border": quad(border_x, border_y, border_w, border_h),
        "margin": quad(margin_x, margin_y, margin_w, margin_h),
        "width": content_w.round() as i64,
        "height": content_h.round() as i64,
    }))
}

/// Serialize a node (and its subtree) back to HTML (best effort)
pub(crate) fn outer_html(doc: &BaseDocument, node_id: NodeId) -> Option<String> {
    let node = doc.get_node(node_id)?;
    let mut out = String::new();
    serialize_node(doc, node, &mut out);
    Some(out)
}

fn serialize_node(doc: &BaseDocument, node: &Node, out: &mut String) {
    match &node.data {
        NodeData::Document(_) => {
            for child in dom_children(doc, node) {
                serialize_node(doc, child, out);
            }
        }
        NodeData::Element(el) | NodeData::AnonymousBlock(el) => {
            let tag = el.name.local.to_string();
            out.push('<');
            out.push_str(&tag);
            for attr in node.attrs().unwrap_or_default() {
                out.push(' ');
                out.push_str(&attr.name.local);
                out.push_str("=\"");
                out.push_str(&attr.value.replace('&', "&amp;").replace('"', "&quot;"));
                out.push('"');
            }
            out.push('>');
            for child in dom_children(doc, node) {
                serialize_node(doc, child, out);
            }
            out.push_str("</");
            out.push_str(&tag);
            out.push('>');
        }
        NodeData::Text(_) => {
            out.push_str(
                &node
                    .text_content()
                    .replace('&', "&amp;")
                    .replace('<', "&lt;"),
            );
        }
        NodeData::Comment => {
            out.push_str("<!---->");
        }
    }
}
