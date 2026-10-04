//! Text selection state and logic for non-input elements.
//!
//! This module handles text selection across inline roots in the document,
//! including support for anonymous blocks which have unstable IDs across
//! layout reconstruction.

use blitz_traits::node_id::NodeId;
use kurbo::{Affine, Point};
use markup5ever::local_name;
use parley::Cluster;
use style::computed_values::visibility::T as Visibility;
use style::values::computed::UserSelect;

use crate::Node;

pub(crate) struct TextPosition {
    pub node_id: NodeId,
    pub offset: usize,
    point: Point,
}

impl Node {
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

    /// Map unscrolled local border-box coordinates to the containing block's coordinates.
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
        let transform = self.transform();
        Affine::translate((x, y))
            * Affine::scale(1.0 / scale)
            * transform.as_deref().copied().unwrap_or(Affine::IDENTITY)
            * Affine::scale(scale)
    }

    /// Find the nearest caret in this layout subtree, even outside its boxes.
    pub(crate) fn nearest_text_position(&self, point: Point, scale: f64) -> Option<TextPosition> {
        if !point.x.is_finite()
            || !point.y.is_finite()
            || self
                .primary_styles()
                .is_some_and(|style| style.clone_display().is_none())
        {
            return None;
        }

        let mut nearest = None;
        let mut distance = f64::INFINITY;
        if self.flags.is_inline_root()
            && let Some(ild) = self.element_data()?.inline_layout_data.as_ref()
            && !ild.text.is_empty()
            && ild.layout.height() > 0.0
        {
            let layout = self.final_layout();
            let origin = Point::new(
                (layout.padding.left + layout.border.left) as f64,
                (layout.padding.top + layout.border.top) as f64,
            );
            let text_point = point - origin.to_vec2();
            let text_scale = ild.layout.scale();
            if let Some((cluster, _)) = Cluster::from_point(
                &ild.layout,
                text_point.x as f32 * text_scale,
                text_point.y as f32 * text_scale,
            ) && self.with(cluster.style().brush.id).text_selection_allowed()
                && let Some(offset) =
                    self.text_offset_at_point(text_point.x as f32, text_point.y as f32)
            {
                let nearest_point = Point::new(
                    text_point.x.clamp(0.0, layout.content_box_width() as f64),
                    text_point
                        .y
                        .clamp(0.0, (ild.layout.height() / text_scale) as f64),
                ) + origin.to_vec2();
                distance = point.distance_squared(nearest_point);
                nearest = Some(TextPosition {
                    node_id: self.id,
                    offset,
                    point: nearest_point,
                });
            }
        }

        let children = self.layout_children.borrow();
        let hoisted = self.hoisted_children.borrow();
        for &child_id in children.iter().flatten().chain(
            hoisted
                .iter()
                .filter(|&&id| self.with(id).layout_parent.get() != Some(self.id)),
        ) {
            let child = self.with(child_id);
            if child.containing_block() != Some(self.id) {
                continue;
            }
            let transform = child.selection_transform_to_parent(scale);
            if let Some(mut position) =
                child.nearest_text_position(transform.inverse() * point, scale)
            {
                position.point = transform * position.point;
                let child_distance = point.distance_squared(position.point);
                if child_distance < distance {
                    distance = child_distance;
                    nearest = Some(position);
                }
            }
        }
        nearest
    }
}

/// Represents one endpoint (anchor or focus) of a text selection.
///
/// For regular nodes, `node_or_parent` contains the node ID directly.
/// For anonymous blocks, `node_or_parent` contains the parent ID and
/// `sibling_index` contains the index among anonymous siblings.
#[derive(Clone, Debug, Default)]
pub struct SelectionEndpoint {
    /// For regular nodes: the node ID directly.
    /// For anonymous blocks: the parent ID (requires lookup via sibling_index).
    pub(crate) node_or_parent: Option<NodeId>,
    /// Byte offset within the inline root's text
    pub offset: usize,
    /// For anonymous blocks only: index among anonymous siblings.
    /// When Some, node_or_parent is a parent ID requiring lookup.
    pub(crate) sibling_index: Option<usize>,
}

impl SelectionEndpoint {
    /// Create a new endpoint at the given node and offset (for non-anonymous nodes)
    fn new(node: NodeId, offset: usize) -> Self {
        Self {
            node_or_parent: Some(node),
            offset,
            sibling_index: None,
        }
    }

    /// Check if this endpoint is set
    pub fn is_some(&self) -> bool {
        self.node_or_parent.is_some()
    }

    /// Clear this endpoint
    pub fn clear(&mut self) {
        self.node_or_parent = None;
        self.offset = 0;
        self.sibling_index = None;
    }

    /// Resolve the node ID, using a lookup function for anonymous blocks.
    pub fn resolve_node_id(
        &self,
        lookup_fn: impl FnOnce(NodeId, usize) -> Option<NodeId>,
    ) -> Option<NodeId> {
        match (self.node_or_parent, self.sibling_index) {
            (Some(node), None) => Some(node),
            (Some(parent), Some(idx)) => lookup_fn(parent, idx),
            _ => None,
        }
    }

    /// Set as a direct node reference (for regular nodes)
    pub fn set_node(&mut self, node: NodeId, offset: usize) {
        self.node_or_parent = Some(node);
        self.offset = offset;
        self.sibling_index = None;
    }

    /// Set as an anonymous block reference
    pub fn set_anonymous(&mut self, parent: NodeId, sibling_index: usize, offset: usize) {
        self.node_or_parent = Some(parent);
        self.offset = offset;
        self.sibling_index = Some(sibling_index);
    }
}

/// Text selection state for non-input elements.
///
/// Tracks both the anchor (where selection started) and focus (where it currently ends).
/// For anonymous blocks, we store stable parent references since anonymous block IDs
/// can change during layout reconstruction.
#[derive(Clone, Debug, Default)]
pub struct TextSelection {
    /// The anchor point (where selection started via mousedown)
    pub anchor: SelectionEndpoint,
    /// The focus point (where selection currently ends, updated during drag)
    pub focus: SelectionEndpoint,
}

impl TextSelection {
    /// Create a selection spanning from anchor to focus
    pub fn new(
        anchor_node: NodeId,
        anchor_offset: usize,
        focus_node: NodeId,
        focus_offset: usize,
    ) -> Self {
        Self {
            anchor: SelectionEndpoint::new(anchor_node, anchor_offset),
            focus: SelectionEndpoint::new(focus_node, focus_offset),
        }
    }

    /// Check if there is an active (non-empty) selection.
    /// Note: For anonymous blocks this compares parent+sibling_index, not resolved node IDs.
    pub fn is_active(&self) -> bool {
        self.anchor.is_some()
            && self.focus.is_some()
            && (self.anchor.node_or_parent != self.focus.node_or_parent
                || self.anchor.sibling_index != self.focus.sibling_index
                || self.anchor.offset != self.focus.offset)
    }

    /// Clear the selection
    pub fn clear(&mut self) {
        self.anchor.clear();
        self.focus.clear();
    }

    /// Update the focus endpoint (for non-anonymous nodes)
    pub fn set_focus(&mut self, node: NodeId, offset: usize) {
        self.focus.set_node(node, offset);
    }
}
