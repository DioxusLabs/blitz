//! The `innerText` getter: an element's rendered text, read back from the inline layouts.
//!
//! Self-contained so that it can move into `blitz-dom` later.

use blitz_dom::node::{ListItemLayoutPosition, Marker, TextBrush, TextLayout};
use blitz_dom::{Node, NodeId};
use markup5ever::local_name;
use style::computed_values::visibility::T as Visibility;
use style::values::computed::Display;
use style::values::specified::box_::{DisplayInside, DisplayOutside};

/// The rendered text of `element`, as returned by `HTMLElement.innerText`.
///
/// Reads the text-transformed and white-space-processed text from the inline layouts,
/// so layout must be up to date. Falls back to `textContent` if the element has no box.
pub(crate) fn inner_text(element: &Node) -> String {
    if !is_rendered(element) {
        return element.text_content();
    }

    // A non-atomic inline in an inline formatting context has no box of its own: collect the
    // text attributed to it from the layouts of its containing block, which may hold several
    // anonymous inline roots. (An inline containing blocks has its own layout children.)
    let mut start = element;
    let mut filter = None;
    if let Some(mut root) = element
        .is_non_atomic_inline()
        .then(|| element.inline_root_ancestor())
        .flatten()
    {
        while root.is_anonymous() && !is_generated_pseudo(root) {
            let Some(parent) = root.layout_parent.get() else {
                break;
            };
            root = element.with(parent);
        }
        start = root;
        filter = Some(element.id);
    }

    let mut collector = InnerTextCollector {
        target: element.id,
        text: String::new(),
        pending_line_breaks: 0,
    };
    collector.visit(start, filter);
    collector.text
}

fn display(node: &Node) -> Option<Display> {
    Some(node.primary_styles()?.clone_display())
}

/// Whether `element` has a box. Stylo drops the styles of `display: none` subtrees, so the
/// only other unrendered elements are the descendants of replaced elements.
fn is_rendered(element: &Node) -> bool {
    let has_box = display(element)
        .is_some_and(|d| !matches!(d.inside(), DisplayInside::None | DisplayInside::Contents));
    let mut ancestors = std::iter::successors(element.parent, |&id| element.with(id).parent);
    element.flags.is_in_document()
        && has_box
        && !ancestors.any(|id| {
            element
                .with(id)
                .element_data()
                .is_some_and(|el| is_replaced_element(&el.name.local))
        })
}

fn is_replaced_element(name: &markup5ever::LocalName) -> bool {
    matches!(
        *name,
        local_name!("img")
            | local_name!("svg")
            | local_name!("canvas")
            | local_name!("video")
            | local_name!("embed")
            | local_name!("iframe")
    )
}

/// `<wbr>` is laid out as a zero-width space, which is not part of the rendered text.
fn is_wbr(node: &Node) -> bool {
    node.element_data()
        .is_some_and(|element| element.name.local == local_name!("wbr"))
}

/// `::before` and `::after` are anonymous nodes referenced by their parent element.
fn is_generated_pseudo(node: &Node) -> bool {
    node.parent.is_some_and(|parent| {
        let parent = node.with(parent);
        parent.before() == Some(node.id) || parent.after() == Some(node.id)
    })
}

fn is_inclusive_descendant_of(node: &Node, ancestor: NodeId) -> bool {
    let mut current = Some(node);
    while let Some(n) = current {
        if n.id == ancestor {
            return true;
        }
        current = n.parent.map(|id| node.with(id));
    }
    false
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
        if is_generated_pseudo(node) {
            return;
        }
        let Some(display) = display(node) else {
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
        if is_inclusive_descendant_of(child, target) {
            self.visit(child, None);
        } else if child.is_anonymous() || is_inclusive_descendant_of(child.with(target), child.id) {
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
            let included = !is_generated_pseudo(node)
                && !is_wbr(node)
                && filter.is_none_or(|target| is_inclusive_descendant_of(node, target))
                && node
                    .primary_styles()
                    .is_none_or(|s| s.clone_visibility() == Visibility::Visible);
            cached_brush = Some((brush.id, included));
            included
        };

        let mut boxes = layout.layout.inline_boxes().peekable();
        let mut runs = Vec::new();
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
        if filter.is_none_or(|target| is_inclusive_descendant_of(node, target)) {
            self.visit(node, None);
        }
    }
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
        .all(|&id| display(cell.with(id)).is_none_or(|d| d.inside() != DisplayInside::TableCell))
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
