//! The `innerText` getter: an element's rendered text, read back from the inline layouts.

use blitz_traits::node_id::NodeId;
use markup5ever::local_name;
use parley::Run;
use style::computed_values::visibility::T as Visibility;
use style::values::specified::box_::{DisplayInside, DisplayOutside};

use crate::layout::replaced::is_replaced_element;

use super::{ListItemLayoutPosition, Marker, Node, TextBrush, TextLayout};

impl Node {
    /// The rendered text of this element, as returned by `HTMLElement.innerText`.
    ///
    /// Reads the text-transformed and white-space-processed text from the inline layouts,
    /// so layout must be up to date. Falls back to `textContent` if the element has no box.
    pub fn inner_text(&self) -> String {
        if !self.is_rendered() {
            return self.text_content();
        }

        // A non-atomic inline in an inline formatting context has no box of its own: collect the
        // text attributed to it from the layouts of its containing block, which may hold several
        // anonymous inline roots. (An inline containing blocks has its own layout children.)
        let mut start = self;
        let mut filter = None;
        if let Some(mut root) = self
            .is_non_atomic_inline()
            .then(|| self.inline_root_ancestor())
            .flatten()
        {
            while root.is_anonymous() && !root.is_generated_pseudo() {
                let Some(parent) = root.layout_parent.get() else {
                    break;
                };
                root = self.with(parent);
            }
            start = root;
            filter = Some(self.id);
        }

        let mut collector = InnerTextCollector {
            target: self.id,
            text: String::new(),
            pending_line_breaks: 0,
        };
        collector.visit(start, filter);
        collector.text
    }

    fn is_rendered(&self) -> bool {
        if !self.flags.is_in_document() || self.element_data().is_none() {
            return false;
        }
        let display = |node: &Node| node.display_style().map(|d| d.inside());
        if matches!(display(self), None | Some(DisplayInside::Contents)) {
            return false;
        }
        let mut node = Some(self);
        while let Some(n) = node {
            if let Some(element) = n.element_data() {
                if matches!(display(n), None | Some(DisplayInside::None)) {
                    return false;
                }
                // The children of replaced elements are not rendered
                if n.id != self.id && is_replaced_element(&element.name.local) {
                    return false;
                }
            }
            node = n.parent.map(|id| self.with(id));
        }
        true
    }

    fn is_generated_pseudo(&self) -> bool {
        self.parent.is_some_and(|parent| {
            let parent = self.with(parent);
            parent.before() == Some(self.id) || parent.after() == Some(self.id)
        })
    }

    fn is_inclusive_descendant_of(&self, ancestor: NodeId) -> bool {
        let mut node = Some(self);
        while let Some(n) = node {
            if n.id == ancestor {
                return true;
            }
            node = n.parent.map(|id| self.with(id));
        }
        false
    }
}

/// Implements the rendered text collection steps, merging required line breaks as it goes.
struct InnerTextCollector {
    target: NodeId,
    text: String,
    pending_line_breaks: usize,
}

impl InnerTextCollector {
    fn push_str(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        if !self.text.is_empty() {
            for _ in 0..self.pending_line_breaks {
                self.text.push('\n');
            }
        }
        self.pending_line_breaks = 0;
        self.text.push_str(s);
    }

    fn require_line_breaks(&mut self, count: usize) {
        self.pending_line_breaks = self.pending_line_breaks.max(count);
    }

    /// Collects the text of `node`'s box. If `filter` is set, only text belonging to that
    /// element (a non-atomic inline inside `node`) is collected.
    fn visit(&mut self, node: &Node, filter: Option<NodeId>) {
        if node.is_generated_pseudo() {
            return;
        }
        let Some(display) = node.display_style() else {
            return;
        };
        if display.inside() == DisplayInside::None {
            return;
        }

        let is_visible = node
            .primary_styles()
            .is_some_and(|s| s.clone_visibility() == Visibility::Visible);
        let line_breaks = match (display.outside(), display.inside()) {
            _ if !is_visible => 0,
            _ if node.data.is_element_with_tag_name(&local_name!("p")) => 2,
            (DisplayOutside::Block | DisplayOutside::TableCaption, _) => 1,
            (_, DisplayInside::TableRow) => 1,
            _ => 0,
        };
        self.require_line_breaks(line_breaks);

        let element_data = node.element_data();
        if let Some(layout) = element_data.and_then(|el| el.inline_layout_data.as_deref()) {
            self.visit_inline_layout(node, layout, filter);
        } else if node.flags.is_table_root() || is_internal_table_part(display.inside()) {
            // Table layout flattens the rows away, so walk the DOM to find them
            for &child_id in &node.children {
                self.visit_child(node.with(child_id), filter);
            }
        } else if let Some(children) = node.layout_children.borrow().as_ref() {
            for &child_id in children {
                self.visit_child(node.with(child_id), filter);
            }
        }

        if display.inside() == DisplayInside::TableCell
            && is_visible
            && node.id != self.target
            && !is_last_table_cell(node)
        {
            self.push_str("\t");
        }
        self.require_line_breaks(line_breaks);
    }

    fn visit_child(&mut self, child: &Node, filter: Option<NodeId>) {
        let Some(target) = filter else {
            return self.visit(child, None);
        };
        if child.is_inclusive_descendant_of(target) {
            self.visit(child, None);
        } else if child.is_anonymous() || node_contains(child, target) {
            self.visit(child, filter);
        }
    }

    fn visit_inline_layout(&mut self, root: &Node, layout: &TextLayout, filter: Option<NodeId>) {
        let text = layout.text.as_str();
        let marker_len = inside_marker_len(root, text);

        let mut cached_brush: Option<(NodeId, bool)> = None;
        let mut include = |brush: &TextBrush| -> bool {
            if let Some((id, included)) = cached_brush {
                if id == brush.id {
                    return included;
                }
            }
            let node = root.with(brush.id);
            let included = !node.is_generated_pseudo()
                && filter.is_none_or(|target| node.is_inclusive_descendant_of(target))
                && node
                    .primary_styles()
                    .is_none_or(|s| s.clone_visibility() == Visibility::Visible);
            cached_brush = Some((brush.id, included));
            included
        };

        let mut boxes = layout.layout.inline_boxes().peekable();
        let mut runs: Vec<Run<'_, TextBrush>> = Vec::new();
        for line in layout.layout.lines() {
            // Runs are stored in visual order: restore logical order for bidi text
            runs.clear();
            runs.extend(line.runs());
            runs.sort_by_key(|run| run.text_range().start);

            for run in &runs {
                for cluster in run.clusters() {
                    let range = cluster.text_range();
                    while let Some(ibox) = boxes.next_if(|ibox| ibox.index <= range.start) {
                        self.visit_inline_box(root, ibox.id, filter);
                    }
                    if range.start >= marker_len && include(&cluster.style().brush) {
                        self.push_str(&text[range]);
                    }
                }
            }
        }
        for ibox in boxes {
            self.visit_inline_box(root, ibox.id, filter);
        }
    }

    fn visit_inline_box(&mut self, root: &Node, id: u64, filter: Option<NodeId>) {
        let node = root.with(NodeId::from_u64(id));
        if filter.is_none_or(|target| node.is_inclusive_descendant_of(target)) {
            self.visit(node, None);
        }
    }
}

fn node_contains(node: &Node, descendant: NodeId) -> bool {
    node.with(descendant).is_inclusive_descendant_of(node.id)
}

fn is_internal_table_part(display: DisplayInside) -> bool {
    matches!(
        display,
        DisplayInside::TableRowGroup
            | DisplayInside::TableHeaderGroup
            | DisplayInside::TableFooterGroup
            | DisplayInside::TableRow
    )
}

fn is_last_table_cell(cell: &Node) -> bool {
    let Some(row) = cell.parent.map(|id| cell.with(id)) else {
        return true;
    };
    let position = row.children.iter().position(|&id| id == cell.id);
    row.children[position.map_or(0, |p| p + 1)..]
        .iter()
        .all(|&id| {
            cell.with(id)
                .display_style()
                .is_none_or(|d| d.inside() != DisplayInside::TableCell)
        })
}

/// The byte length of a `list-style-position: inside` marker at the start of `text`.
fn inside_marker_len(root: &Node, text: &str) -> usize {
    let Some(list_item) = root
        .element_data()
        .and_then(|el| el.list_item_data.as_deref())
    else {
        return 0;
    };
    if !matches!(list_item.position, ListItemLayoutPosition::Inside) {
        return 0;
    }
    let len = match &list_item.marker {
        Marker::Char(c) => c.len_utf8() + 1,
        Marker::String(s) => s.len(),
    };
    if text.is_char_boundary(len.min(text.len())) {
        len.min(text.len())
    } else {
        0
    }
}
