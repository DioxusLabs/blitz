//! Document selections use layout offsets in text and DOM child boundaries elsewhere.

use crate::{BaseDocument, Node, node::TextLayout};
use blitz_traits::node_id::NodeId;
use kurbo::{Affine, Point};
use markup5ever::local_name;
use std::{cmp::Ordering, ops::Range};
use style::computed_values::visibility::T as Visibility;
use style::values::computed::UserSelect;

/// The type of position held by a selection endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionKind {
    Text,
    Element,
    AnonymousText { index: usize },
}

/// Text positions use an inline root's flattened UTF-8 layout bytes; element
/// positions count its DOM children. Anonymous roots use a stable parent/index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionPoint {
    pub node: NodeId,
    pub offset: usize,
    pub kind: SelectionKind,
}

impl SelectionPoint {
    pub fn text(node: NodeId, offset: usize) -> Self {
        Self {
            node,
            offset,
            kind: SelectionKind::Text,
        }
    }
    pub fn element(node: NodeId, offset: usize) -> Self {
        Self {
            node,
            offset,
            kind: SelectionKind::Element,
        }
    }
}

/// Anchor and focus, each in either text-layout or element coordinates.
#[derive(Clone, Debug, Default)]
pub struct TextSelection {
    pub anchor: Option<SelectionPoint>,
    pub focus: Option<SelectionPoint>,
}

impl TextSelection {
    pub fn is_active(&self) -> bool {
        self.anchor.is_some() && self.focus.is_some() && self.anchor != self.focus
    }
    pub fn clear(&mut self) {
        self.anchor = None;
        self.focus = None;
    }
}

impl TextLayout {
    pub(crate) fn rebuild_source_ranges(&mut self) {
        let mut ranges: Vec<_> = self
            .layout
            .lines()
            .flat_map(|line| line.runs())
            .flat_map(|run| run.clusters())
            .filter_map(|cluster| Some((cluster.text_range(), cluster.style().brush.text_node?)))
            .collect();
        ranges.sort_by_key(|(range, _)| range.start);
        ranges.dedup_by_key(|(range, _)| range.start);
        self.source_ranges.clear();
        for (range, source) in ranges {
            if let Some((last, last_source)) = self.source_ranges.last_mut()
                && *last_source == source
                && last.end == range.start
            {
                last.end = range.end;
            } else {
                self.source_ranges.push((range, source));
            }
        }
    }

    pub(crate) fn source_ranges(&self) -> &[(Range<usize>, NodeId)] {
        &self.source_ranges
    }
}

impl Node {
    pub(crate) fn selection_text_source_at_point(&self, x: f32, y: f32) -> Option<NodeId> {
        let layout = &self.element_data()?.inline_layout_data.as_ref()?.layout;
        parley::layout::Cluster::from_point(layout, x * layout.scale(), y * layout.scale())
            .and_then(|(cluster, _)| cluster.style().brush.text_node)
    }

    pub(crate) fn selection_point_in_text(&self, x: f32, y: f32) -> Option<SelectionPoint> {
        let offset = self.text_offset_at_point(x, y)?;
        let ild = self.element_data()?.inline_layout_data.as_ref()?;
        let layout = &ild.layout;
        let mut point = SelectionPoint::text(self.id, offset);
        let source = self.selection_text_source_at_point(x, y);
        for inline_box in layout.inline_boxes().filter(|b| b.index == offset) {
            let Some(source) = source else {
                break;
            };
            let Some(child) = self.tree().get(NodeId::from_u64(inline_box.id)) else {
                continue;
            };
            // Inline boxes occupy no text bytes, so their two edges need child boundaries.
            let Some(source) = self.tree().get(source) else {
                continue;
            };
            let order = source.compare_document_order(child);
            if order.is_lt() {
                return child.dom_edge(false);
            }
            if order.is_gt() {
                point = child.dom_edge(true)?;
            }
        }
        Some(point)
    }

    pub(crate) fn dom_edge(&self, after: bool) -> Option<SelectionPoint> {
        let parent = self.with(self.parent?);
        if parent.before() == Some(self.id) {
            return Some(SelectionPoint::element(parent.id, 0));
        }
        if parent.after() == Some(self.id) {
            return Some(SelectionPoint::element(parent.id, parent.children.len()));
        }
        if self.is_anonymous() {
            let child = if after {
                self.children.last()
            } else {
                self.children.first()
            };
            return child
                .and_then(|id| self.with(*id).dom_edge(after))
                .or_else(|| Some(SelectionPoint::element(self.parent?, 0)));
        }
        if let Some(index) = parent.children.iter().position(|id| *id == self.id) {
            Some(SelectionPoint::element(
                parent.id,
                index + usize::from(after),
            ))
        } else {
            Some(SelectionPoint::element(parent.id, 0))
        }
    }

    pub(crate) fn selection_child_boundary(
        &self,
        point: Point,
        scale: f64,
    ) -> Option<SelectionPoint> {
        for &id in self
            .layout_children
            .borrow()
            .iter()
            .flatten()
            .chain(self.hoisted_children.borrow().iter())
        {
            let child = self.with(id);
            if child.containing_block() != Some(self.id) {
                continue;
            }
            let local = child.selection_transform_to_parent(scale).inverse() * point;
            let size = child.final_layout().size;
            if local.x >= 0.0
                && local.x <= size.width as f64
                && local.y >= 0.0
                && local.y <= size.height as f64
            {
                if let Some(position) = child.selection_child_boundary(local, scale) {
                    return Some(position);
                }
            }
        }
        let mut end = None;
        let layout = self.final_layout();
        let content_top = (layout.padding.top + layout.border.top) as f64;
        let content_left = (layout.padding.left + layout.border.left) as f64;
        let x = (point.x - content_left) as f32;
        let y = (point.y - content_top) as f32;
        if self.text_selection_allowed()
            && let Some(ild) = self
                .element_data()
                .and_then(|el| el.inline_layout_data.as_ref())
            && x >= 0.0
            && x <= layout.content_box_width()
            && y >= 0.0
            && y < ild.layout.height() / ild.layout.scale()
            && let Some(position) = self.selection_point_in_text(x, y)
            && !ild.text.is_empty()
        {
            return Some(position);
        }
        let at_start = point.y <= content_top || point.x <= content_left;
        for &id in self.layout_children.borrow().iter().flatten() {
            let child = self.with(id);
            if child.taffy_position().is_out_of_flow() {
                continue;
            }
            let local = child.selection_transform_to_parent(scale).inverse() * point;
            let size = child.final_layout().size;
            if local.y < size.height as f64 && (local.x < size.width as f64 || local.y < 0.0) {
                return child.dom_edge(false);
            }
            end = child.dom_edge(true);
        }
        if self.is_anonymous() {
            self.dom_edge(!at_start)
        } else {
            end.or(Some(SelectionPoint::element(
                self.id,
                if at_start { 0 } else { self.children.len() },
            )))
        }
    }

    pub(crate) fn text_selection_allowed(&self) -> bool {
        let mut node = self;
        let mut user_select = UserSelect::Auto;
        let mut visibility = None;
        loop {
            if let Some(el) = node.element_data() {
                if el.has_attr(local_name!("disabled"))
                    || matches!(
                        el.name.local,
                        local_name!("input")
                            | local_name!("textarea")
                            | local_name!("select")
                            | local_name!("button")
                    )
                    || node.subdoc().is_some()
                {
                    return false;
                }
            }
            if let Some(style) = node.primary_styles() {
                visibility.get_or_insert_with(|| style.clone_visibility());
                if user_select == UserSelect::Auto {
                    user_select = style.clone_user_select();
                }
            }
            let Some(parent) = node.parent else {
                return user_select != UserSelect::None
                    && !matches!(visibility, Some(Visibility::Hidden | Visibility::Collapse));
            };
            node = self.with(parent);
        }
    }

    pub(crate) fn selection_transform_to_parent(&self, scale: f64) -> Affine {
        let layout = self.final_layout();
        let mut x = layout.location.x as f64 - self.scroll_offset().x;
        let mut y = layout.location.y as f64 - self.scroll_offset().y;
        if let Some(parent_id) = self.containing_block() {
            let parent = self.with(parent_id);
            if parent.flags.is_inline_root() && !self.taffy_position().is_out_of_flow() {
                let layout = parent.final_layout();
                x += (layout.padding.left + layout.border.left) as f64;
                y += (layout.padding.top + layout.border.top) as f64;
            }
            if self.taffy_position() == taffy::Position::Fixed
                && parent.containing_block().is_none()
            {
                x += parent.scroll_offset().x + self.tree().viewport_scroll().x;
                y += parent.scroll_offset().y + self.tree().viewport_scroll().y;
            }
        }
        Affine::translate((x, y))
            * Affine::scale(1.0 / scale)
            * self
                .transform()
                .as_deref()
                .copied()
                .unwrap_or(Affine::IDENTITY)
            * Affine::scale(scale)
    }
}

impl BaseDocument {
    pub(crate) fn normalize_selection_point(&self, point: SelectionPoint) -> SelectionPoint {
        if point.kind != SelectionKind::Text {
            return point;
        }
        let Some(root) = self.get_node(point.node) else {
            return point;
        };
        if !root.is_anonymous() {
            return point;
        }
        let Some(parent_id) = root.parent else {
            return point;
        };
        let parent = &self.nodes[parent_id];
        let children = parent.layout_children.borrow();
        let Some(index) = children
            .iter()
            .flatten()
            .filter(|&&id| self.nodes[id].is_anonymous())
            .position(|&id| id == point.node)
        else {
            return point;
        };
        SelectionPoint {
            node: parent_id,
            offset: point.offset,
            kind: SelectionKind::AnonymousText { index },
        }
    }

    pub(crate) fn resolve_selection_point(&self, point: SelectionPoint) -> Option<SelectionPoint> {
        if let SelectionKind::AnonymousText { index } = point.kind {
            let parent = self.get_node(point.node)?;
            let children = parent.layout_children.borrow();
            let root = children
                .iter()
                .flatten()
                .filter(|&&id| self.nodes[id].is_anonymous())
                .nth(index)?;
            Some(SelectionPoint::text(*root, point.offset))
        } else {
            Some(point)
        }
    }

    pub(crate) fn valid_selection_point(&self, point: SelectionPoint) -> bool {
        let Some(resolved) = self.resolve_selection_point(point) else {
            return false;
        };
        if resolved.kind == SelectionKind::Text {
            return self.get_node(resolved.node).is_some_and(|node| {
                node.flags.is_in_document()
                    && node.flags.is_inline_root()
                    && node
                        .element_data()
                        .and_then(|el| el.inline_layout_data.as_ref())
                        .is_some_and(|ild| ild.text.is_char_boundary(resolved.offset))
            });
        }
        self.valid_dom_node(point.node)
            && self.get_node(point.node).is_some_and(|node| {
                node.text_data().is_none() && point.offset <= node.children.len()
            })
    }

    pub(crate) fn valid_dom_node(&self, id: NodeId) -> bool {
        self.get_node(id).is_some_and(|node| {
            let mut current = node;
            while let Some(parent_id) = current.parent {
                let Some(parent) = self.get_node(parent_id) else {
                    return false;
                };
                if !parent.children.contains(&current.id) {
                    return false;
                }
                current = parent;
            }
            !node.is_anonymous() && current.id == self.root_node().id && node.flags.is_in_document()
        })
    }

    pub(crate) fn text_point_in_dom(&self, point: SelectionPoint) -> Option<SelectionPoint> {
        let point = self.resolve_selection_point(point)?;
        if point.kind == SelectionKind::Element {
            return Some(point);
        }
        let root = self.get_node(point.node)?;
        let layout = root.element_data()?.inline_layout_data.as_ref()?;
        let ranges = layout.source_ranges();
        let source = ranges
            .iter()
            .find(|(range, id)| range.end > point.offset && self.valid_dom_node(*id))
            .or_else(|| ranges.iter().rev().find(|(_, id)| self.valid_dom_node(*id)));
        let Some((range, id)) = source else {
            return root.dom_edge(point.offset == layout.text.len());
        };
        if point.offset >= range.end {
            self.nodes[*id].dom_edge(true)
        } else {
            Some(SelectionPoint::element(*id, 0))
        }
    }

    pub(crate) fn compare_dom_boundaries(&self, a: SelectionPoint, b: SelectionPoint) -> Ordering {
        if a.node == b.node {
            return a.offset.cmp(&b.offset);
        }
        let child_index = |ancestor, mut child| {
            while let Some(node) = self.get_node(child) {
                let parent = node.parent?;
                if parent == ancestor {
                    return self.nodes[parent]
                        .children
                        .iter()
                        .position(|id| *id == child);
                }
                child = parent;
            }
            None
        };
        if let Some(index) = child_index(a.node, b.node) {
            return if a.offset <= index {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        if let Some(index) = child_index(b.node, a.node) {
            return if b.offset <= index {
                Ordering::Greater
            } else {
                Ordering::Less
            };
        }
        self.compare_document_order(a.node, b.node)
    }

    pub(crate) fn compare_selection_points(
        &self,
        a: SelectionPoint,
        b: SelectionPoint,
    ) -> Ordering {
        let a = self.resolve_selection_point(a).unwrap_or(a);
        let b = self.resolve_selection_point(b).unwrap_or(b);
        if a.kind == SelectionKind::Text && b.kind == SelectionKind::Text && a.node == b.node {
            return a.offset.cmp(&b.offset);
        }
        let a = self.text_point_in_dom(a).unwrap_or(a);
        let b = self.text_point_in_dom(b).unwrap_or(b);
        self.compare_dom_boundaries(a, b)
    }
}
