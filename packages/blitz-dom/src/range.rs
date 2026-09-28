//! Geometry of DOM ranges (`Range.getClientRects()` / `getBoundingClientRect()`)

use markup5ever::local_name;
use parley::{Affinity, Cursor, Selection};
use style::values::computed::{Display, TextTransform};
use style::values::specified::box_::{DisplayInside, DisplayOutside};

use crate::document::{BoundingRect, snap_to_layout_unit};
use crate::layout::replaced::is_replaced_element;
use crate::node::NodeData;
use crate::{BaseDocument, NodeId};

/// A DOM range boundary point: a node and an offset within it (a child index for
/// non-text nodes, a UTF-16 code unit offset for text nodes).
pub type BoundaryPoint = (NodeId, usize);

impl BaseDocument {
    /// Computes the rects of a DOM range relative to the viewport (CSSOM View's
    /// `Range.getClientRects()`): the client rects of each element contained in the
    /// range whose parent is not contained, plus the rects of the selected portion
    /// of each text node selected (or partially selected) by the range.
    pub fn range_client_rects(
        &self,
        start: BoundaryPoint,
        end: BoundaryPoint,
    ) -> Vec<BoundingRect> {
        let Some(root_id) = self.dom_root(start.0) else {
            return Vec::new();
        };

        // Pre-order traversal with "enter" and "exit" positions, so that boundary points
        // and node extents can be compared as integers. Text boundary points lie strictly
        // between their text node's enter and exit positions.
        let mut order: Vec<(NodeId, u64, u64)> = Vec::new();
        let mut positions: std::collections::HashMap<NodeId, (u64, u64)> = Default::default();
        let mut counter = 0u64;
        self.number_subtree(root_id, &mut counter, &mut order, &mut positions);

        let boundary_pos = |(node_id, offset): BoundaryPoint| -> Option<u64> {
            let node = self.get_node(node_id)?;
            let (enter, exit) = *positions.get(&node_id)?;
            if node.is_text_node() || matches!(node.data, NodeData::Comment { .. }) {
                return Some(enter + 1);
            }
            Some(match node.children.get(offset) {
                Some(child) => positions.get(child)?.0,
                None => exit,
            })
        };
        let (Some(start_pos), Some(end_pos)) = (boundary_pos(start), boundary_pos(end)) else {
            return Vec::new();
        };
        let is_contained = |node_id: NodeId| {
            positions
                .get(&node_id)
                .is_some_and(|&(enter, exit)| start_pos <= enter && exit <= end_pos)
        };

        let mut rects = Vec::new();
        for &(node_id, enter, exit) in &order {
            if enter < start_pos && exit < start_pos || enter > end_pos {
                continue;
            }
            let node = &self.nodes[node_id];
            if node.is_element() {
                let parent_contained = node.parent.is_some_and(is_contained);
                if is_contained(node_id) && !parent_contained && node.has_boxes() {
                    rects.extend(self.node_client_rects(node_id));
                }
            } else if let Some(text) = node.text_data() {
                let utf16_len = text.content.encode_utf16().count();
                let from = if node_id == start.0 { start.1 } else { 0 };
                let to = if node_id == end.0 { end.1 } else { utf16_len };
                if node_id == start.0 || node_id == end.0 || is_contained(node_id) {
                    rects.extend(self.text_range_rects(node_id, from, to));
                }
            }
        }
        rects
    }

    /// The root of the DOM tree containing `node_id`
    fn dom_root(&self, mut node_id: NodeId) -> Option<NodeId> {
        loop {
            match self.get_node(node_id)?.parent {
                Some(parent) => node_id = parent,
                None => return Some(node_id),
            }
        }
    }

    fn number_subtree(
        &self,
        node_id: NodeId,
        counter: &mut u64,
        order: &mut Vec<(NodeId, u64, u64)>,
        positions: &mut std::collections::HashMap<NodeId, (u64, u64)>,
    ) {
        let enter = *counter;
        // Leave room for text boundary points between enter and exit
        *counter += 2;
        let index = order.len();
        order.push((node_id, enter, 0));
        for &child_id in &self.nodes[node_id].children {
            self.number_subtree(child_id, counter, order, positions);
        }
        let exit = *counter;
        *counter += 1;
        order[index].2 = exit;
        positions.insert(node_id, (enter, exit));
    }

    /// Rects (one per line box) of the text between UTF-16 offsets `from` and `to` of a
    /// text node, relative to the viewport. A collapsed range yields a zero-width rect.
    fn text_range_rects(&self, text_id: NodeId, from: usize, to: usize) -> Vec<BoundingRect> {
        let Some(text_node) = self.get_node(text_id) else {
            return Vec::new();
        };
        if !text_node.flags.is_in_document() {
            return Vec::new();
        }
        let Some(inline_root) = text_node.inline_root_ancestor() else {
            return Vec::new();
        };
        let Some(inline_layout) = inline_root
            .element_data()
            .and_then(|el| el.inline_layout_data.as_ref())
        else {
            return Vec::new();
        };
        let Some((byte_start, byte_end)) =
            self.text_node_layout_range(inline_root.id, &inline_layout.text, text_id, from, to)
        else {
            return Vec::new();
        };

        let layout = &inline_layout.layout;
        let scale = layout.scale() as f64;

        // Rects are relative to the inline root's content box
        let root_layout = inline_root.unrounded_layout();
        let root_pos = inline_root.unrounded_absolute_position(0.0, 0.0);
        let origin_x = root_pos.x as f64
            + (root_layout.padding.left + root_layout.border.left) as f64
            - self.viewport_scroll().x;
        let origin_y = root_pos.y as f64
            + (root_layout.padding.top + root_layout.border.top) as f64
            - self.viewport_scroll().y;
        let to_client_rect = |x0: f64, y0: f64, x1: f64, y1: f64| BoundingRect {
            x: snap_to_layout_unit(origin_x + x0 / scale),
            y: snap_to_layout_unit(origin_y + y0 / scale),
            width: snap_to_layout_unit((x1 - x0) / scale),
            height: snap_to_layout_unit((y1 - y0) / scale),
        };

        if from == to {
            let cursor = Cursor::from_byte_index(layout, byte_start, Affinity::Downstream);
            let caret = cursor.geometry(layout, 0.0);
            return vec![to_client_rect(caret.x0, caret.y0, caret.x0, caret.y1)];
        }
        if byte_start >= byte_end {
            // The text was entirely collapsed away
            return Vec::new();
        }

        let anchor = Cursor::from_byte_index(layout, byte_start, Affinity::Downstream);
        let focus = Cursor::from_byte_index(layout, byte_end, Affinity::Downstream);
        let mut line_rects: Vec<(usize, (f64, f64, f64, f64))> = Vec::new();
        Selection::new(anchor, focus).geometry_with(layout, |rect, line_idx| {
            match line_rects.iter_mut().find(|(idx, _)| *idx == line_idx) {
                Some((_, (x0, y0, x1, y1))) => {
                    *x0 = x0.min(rect.x0);
                    *y0 = y0.min(rect.y0);
                    *x1 = x1.max(rect.x1);
                    *y1 = y1.max(rect.y1);
                }
                None => line_rects.push((line_idx, (rect.x0, rect.y0, rect.x1, rect.y1))),
            }
        });
        line_rects
            .into_iter()
            .map(|(_, (x0, y0, x1, y1))| to_client_rect(x0, y0, x1, y1))
            .collect()
    }

    /// Maps the UTF-16 offsets `from..to` within text node `text_id` to a byte range
    /// within its inline root's layout text.
    ///
    /// The layout text is the concatenation of the inline root's text nodes after
    /// whitespace collapsing and text-transform, so the text nodes' contents are
    /// aligned against it character by character: collapsible whitespace which is
    /// missing from the layout text is skipped, and layout text which doesn't come
    /// from a text node (list markers, `<br>` newlines) is skipped over.
    fn text_node_layout_range(
        &self,
        inline_root_id: NodeId,
        layout_text: &str,
        text_id: NodeId,
        from: usize,
        to: usize,
    ) -> Option<(usize, usize)> {
        let mut segments = Vec::new();
        let root = &self.nodes[inline_root_id];
        let text_transform = root
            .primary_styles()
            .map(|s| s.clone_text_transform() & TextTransform::CASE_TRANSFORMS)
            .unwrap_or(TextTransform::NONE);
        for child_id in root
            .before()
            .into_iter()
            .chain(root.children.iter().copied())
            .chain(root.after())
        {
            self.collect_inline_text_segments(child_id, text_transform, &mut segments);
        }

        let mut layout_pos = 0;
        for (node_id, text) in segments {
            let is_target = node_id == text_id;
            let mut offsets = Vec::new();
            for c in text.chars() {
                let rest = &layout_text[layout_pos..];
                if is_target {
                    offsets.push((c.len_utf16(), layout_pos));
                }
                if matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C') {
                    // Collapsible whitespace: consumes a layout whitespace character
                    // if there is one, otherwise it was collapsed away
                    if let Some(lc) = rest.chars().next().filter(|lc| lc.is_whitespace()) {
                        layout_pos += lc.len_utf8();
                    }
                } else if let Some(idx) = rest.find(c) {
                    if is_target {
                        offsets.last_mut().unwrap().1 = layout_pos + idx;
                    }
                    layout_pos += idx + c.len_utf8();
                }
            }
            if is_target {
                let utf16_to_byte = |utf16_offset: usize| {
                    let mut utf16_pos = 0;
                    for &(len, byte) in &offsets {
                        if utf16_pos + len > utf16_offset {
                            return byte;
                        }
                        utf16_pos += len;
                    }
                    layout_pos
                };
                return Some((utf16_to_byte(from), utf16_to_byte(to.max(from))));
            }
        }
        None
    }

    /// Collect the (text-transformed) text nodes which contribute to an inline
    /// formatting context, in the same order as inline layout construction.
    fn collect_inline_text_segments(
        &self,
        node_id: NodeId,
        parent_text_transform: TextTransform,
        segments: &mut Vec<(NodeId, String)>,
    ) {
        let node = &self.nodes[node_id];
        match &node.data {
            NodeData::Element(element) | NodeData::AnonymousBlock(element) => {
                let text_transform = node
                    .primary_styles()
                    .map(|s| s.clone_text_transform() & TextTransform::CASE_TRANSFORMS)
                    .unwrap_or(TextTransform::NONE);
                let display = node.display_style().unwrap_or(Display::inline());
                let recurse = match (display.outside(), display.inside()) {
                    (DisplayOutside::None, DisplayInside::Contents) => true,
                    (DisplayOutside::Inline, DisplayInside::Flow) => {
                        let tag = &element.name.local;
                        !(is_replaced_element(tag)
                            || *tag == local_name!("input")
                            || *tag == local_name!("textarea")
                            || *tag == local_name!("button")
                            || *tag == local_name!("br"))
                    }
                    _ => false,
                };
                if recurse {
                    for child_id in node
                        .before()
                        .into_iter()
                        .chain(node.children.iter().copied())
                        .chain(node.after())
                    {
                        self.collect_inline_text_segments(child_id, text_transform, segments);
                    }
                }
            }
            NodeData::Text(data) => {
                let text = match parent_text_transform {
                    TextTransform::UPPERCASE => data.content.to_uppercase(),
                    TextTransform::LOWERCASE => data.content.to_lowercase(),
                    _ => data.content.clone(),
                };
                segments.push((node_id, text));
            }
            _ => {}
        }
    }
}
