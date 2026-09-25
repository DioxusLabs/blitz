//! Construction of the paint tree (`Node::paint_children` and
//! `Node::stacking_context`) from the laid-out box tree.
//!
//! The pass runs once per frame after layout and builds *topology* only: which
//! node ids each node paints itself and which z-indexed descendants are hoisted
//! to the nearest stacking-context root. Geometry (a hoisted box's offset from
//! its stacking-context root) is derived at use time from `final_layout()` and
//! memoised per geometry generation, so replaying a cached list for a clean
//! subtree can never paint or hit-test a box where layout no longer puts it.

use blitz_traits::node_id::NodeId;
use std::cell::Cell;
use std::ops::Range;

use crate::layout::damage::ALL_DAMAGE;
use crate::tree::NodeTree;
use crate::{BaseDocument, Node};
use style::properties::generated::longhands::position::computed_value::T as Position;
use style::selector_parser::RestyleDamage;
use style::values::computed::Float;
use style::values::specified::box_::DisplayInside;
use taffy::{Point, Rect};

/// Offset of the hoisted child `child_id` from the border box of its
/// stacking-context root `sc_root_id`, excluding the child's own
/// `final_layout().location`: the sum of `location - scroll_offset` of every
/// layout ancestor strictly between the two.
///
/// Walks `layout_parent`, which is set for every layout child by
/// `resolve_layout_children` (anonymous blocks included).
pub fn hoisted_child_position(tree: &NodeTree, sc_root_id: NodeId, child_id: NodeId) -> Point<f32> {
    let mut position = Point::ZERO;
    let mut current = tree[child_id].layout_parent.get();
    while let Some(id) = current {
        if id == sc_root_id {
            break;
        }
        let Some(node) = tree.get(id) else {
            break;
        };
        let location = node.final_layout().location;
        let scroll_offset = *node.scroll_offset();
        position.x += location.x - scroll_offset.x as f32;
        position.y += location.y - scroll_offset.y as f32;
        current = node.layout_parent.get();
    }
    position
}

/// A child with a z_index that is hoisted up to it's containing Stacking Context for paint purposes
#[derive(Debug, Clone)]
pub struct HoistedPaintChild {
    pub node_id: NodeId,
    pub z_index: i32,
    /// `(geometry generation, offset from the stacking-context root)`, see
    /// [`Self::position`]. Generation `0` is never current.
    position: Cell<(u64, Point<f32>)>,
}

impl HoistedPaintChild {
    fn new(node_id: NodeId, z_index: i32) -> Self {
        Self {
            node_id,
            z_index,
            position: Cell::new((0, Point::ZERO)),
        }
    }

    /// The child's offset from the border box of its stacking-context root
    /// (`sc_root_id`), excluding the child's own `final_layout().location`.
    /// Computed at most once per geometry generation.
    pub fn position(&self, tree: &NodeTree, sc_root_id: NodeId) -> Point<f32> {
        let generation = tree.geometry_generation();
        let (stamp, cached) = self.position.get();
        if stamp == generation {
            return cached;
        }
        let position = hoisted_child_position(tree, sc_root_id, self.node_id);
        self.position.set((generation, position));
        position
    }

    /// Whether [`Self::position`] is memoised for the current geometry generation.
    pub fn position_is_cached(&self, tree: &NodeTree) -> bool {
        self.position.get().0 == tree.geometry_generation()
    }
}

#[derive(Debug)]
pub struct StackingContext {
    pub children: Vec<HoistedPaintChild>,
    /// The number of hoisted point children with negative z_index
    pub negative_z_count: u32,
    /// `(geometry generation, bounding box of the hoisted children)`, see
    /// [`Self::hoisted_content_bbox`].
    hoisted_content_bbox: Cell<(u64, Rect<f32>)>,
}

impl StackingContext {
    fn new() -> Self {
        Self {
            children: Vec::new(),
            negative_z_count: 0,
            hoisted_content_bbox: Cell::new((0, Rect::ZERO)),
        }
    }

    /// Bounding box (relative to the stacking-context root `sc_root_id`,
    /// unscrolled) of all hoisted children. Computed at most once per
    /// geometry generation.
    pub fn hoisted_content_bbox(&self, tree: &NodeTree, sc_root_id: NodeId) -> Rect<f32> {
        let generation = tree.geometry_generation();
        let (stamp, cached) = self.hoisted_content_bbox.get();
        if stamp == generation {
            return cached;
        }

        let child_rect = |child: &HoistedPaintChild| -> Rect<f32> {
            let node = &tree[child.node_id];
            let position = child.position(tree, sc_root_id);
            let layout = node.final_layout();
            let left = position.x + layout.location.x;
            let top = position.y + layout.location.y;
            Rect {
                left,
                top,
                right: left + layout.size.width,
                bottom: top + layout.size.height,
            }
        };

        let mut area = Rect::ZERO;
        if let Some((first, rest)) = self.children.split_first() {
            area = child_rect(first);
            for child in rest {
                let pos = child_rect(child);
                area.left = area.left.min(pos.left);
                area.top = area.top.min(pos.top);
                area.right = area.right.max(pos.right);
                area.bottom = area.bottom.max(pos.bottom);
            }
        }
        self.hoisted_content_bbox.set((generation, area));
        area
    }

    pub fn sort(&mut self) {
        self.children.sort_by_key(|c| c.z_index);
        self.negative_z_count = self.children.iter().take_while(|c| c.z_index < 0).count() as u32;
    }

    pub fn neg_z_range(&self) -> Range<usize> {
        0..(self.negative_z_count as usize)
    }

    pub fn pos_z_range(&self) -> Range<usize> {
        (self.negative_z_count as usize)..self.children.len()
    }

    pub fn neg_z_hoisted_children(
        &self,
    ) -> impl ExactSizeIterator<Item = &HoistedPaintChild> + DoubleEndedIterator {
        self.children[self.neg_z_range()].iter()
    }

    pub fn pos_z_hoisted_children(
        &self,
    ) -> impl ExactSizeIterator<Item = &HoistedPaintChild> + DoubleEndedIterator {
        self.children[self.pos_z_range()].iter()
    }
}

impl BaseDocument {
    /// Rebuild `paint_children` and `stacking_context` for the tree rooted at
    /// `root_id` from the laid-out box tree.
    ///
    /// Each node's layout children are placed either in its `paint_children`
    /// (sorted by paint order) or, when they have a non-zero `z-index` and are
    /// positioned or flex/grid items, in the nearest ancestor stacking context.
    ///
    /// Subtrees whose stored damage is at most `REPAINT` are skipped: their
    /// own lists are still valid and the entries they contributed to the
    /// enclosing stacking context are replayed from `Node::sc_contribution_cache`.
    pub(crate) fn build_paint_tree(&mut self, root_id: NodeId) {
        self.build_paint_tree_impl(root_id, None);
    }

    fn build_paint_tree_impl(
        &mut self,
        node_id: NodeId,
        parent_stacking_context: Option<&mut StackingContext>,
    ) {
        {
            let node = &self.nodes[node_id];
            // `propagate_damage_flags` stores the union of a node's own and
            // its descendants' damage on the node, so `REPAINT`-only means
            // that nothing in the subtree can have changed a paint list.
            // Anonymous boxes are never skipped: damage marking walks the DOM
            // parent chain, which bypasses them.
            //
            // Whether a node is a stacking-context root also depends on its
            // parent (z-indexed flex/grid items), which does not damage the
            // node itself, so the role the caller resolved must match the one
            // this node was last built with.
            let role_unchanged =
                node.stacking_context.is_some() == parent_stacking_context.is_none();
            let clean = role_unchanged
                && node
                    .damage()
                    .unwrap_or(ALL_DAMAGE)
                    .difference(RestyleDamage::REPAINT)
                    .is_empty()
                && !node.is_anonymous();
            if clean {
                if let Some(parent_stacking_context) = parent_stacking_context {
                    parent_stacking_context
                        .children
                        .extend(node.sc_contribution_cache.borrow().iter().cloned());
                }
                return;
            }
        }
        let mut new_stacking_context = StackingContext::new();
        let stacking_context = &mut new_stacking_context;

        let Some(display) = self.nodes[node_id].display_style() else {
            return;
        };

        let children = self.nodes[node_id].layout_children.borrow_mut().take();
        if let Some(children) = children {
            let is_flex_or_grid =
                matches!(display.inside(), DisplayInside::Flex | DisplayInside::Grid);

            for &child in children.iter() {
                self.build_paint_tree_impl(
                    child,
                    match self.nodes[child].is_stacking_context_root(is_flex_or_grid) {
                        true => None,
                        false => Some(stacking_context),
                    },
                );
            }

            let mut paint_children = self.nodes[node_id]
                .paint_children
                .borrow_mut()
                .take()
                .unwrap_or_default();
            paint_children.clear();
            paint_children.reserve(children.len());

            for &child_id in children.iter() {
                let child = &self.nodes[child_id];

                let Some(style) = child.primary_styles() else {
                    paint_children.push(child_id);
                    continue;
                };

                let position = style.clone_position();
                let z_index = style.clone_z_index().integer_or(0);

                // TODO: more complete hoisting detection
                // z-index applies to static flex/grid items too
                // (css-flexbox-1 §painting, css-grid-1 §z-order).
                if z_index != 0 && (position != Position::Static || is_flex_or_grid) {
                    stacking_context
                        .children
                        .push(HoistedPaintChild::new(child_id, z_index))
                } else {
                    paint_children.push(child_id);
                }
            }

            paint_children
                .sort_by_cached_key(|id| node_to_paint_order(&self.nodes[*id], is_flex_or_grid));

            let node = &self.nodes[node_id];
            *node.paint_children.borrow_mut() = Some(paint_children);
            *node.layout_children.borrow_mut() = Some(children);
        }

        let node = &mut self.nodes[node_id];
        if let Some(parent_stacking_context) = parent_stacking_context {
            node.stacking_context = None;
            let mut cache = node.sc_contribution_cache.borrow_mut();
            cache.clear();
            cache.extend(stacking_context.children.iter().cloned());
            parent_stacking_context
                .children
                .append(&mut stacking_context.children);
        } else {
            node.sc_contribution_cache.borrow_mut().clear();
            stacking_context.sort();
            node.stacking_context = Some(Box::new(new_stacking_context));
        }
    }
}

#[inline(always)]
fn position_to_order(pos: Position) -> i32 {
    match pos {
        Position::Static => 0,
        // All positioned descendants with z-index: auto share one paint
        // level (CSS 2.1 Appendix E step 8); the stable sort keeps them in
        // tree order among themselves, above in-flow content and floats.
        Position::Relative | Position::Sticky | Position::Absolute | Position::Fixed => 2,
    }
}
#[inline(always)]
fn float_to_order(pos: Float) -> i32 {
    match pos {
        Float::None => 0,
        _ => 1,
    }
}

/// Paint sort key: (paint level, order-modified position). Positioned
/// (z-index: auto) descendants paint above in-flow content (CSS 2.1
/// Appendix E step 8); within a level the stable sort preserves
/// (order-modified) document order.
#[inline(always)]
fn node_to_paint_order(node: &Node, is_flex_or_grid: bool) -> (i32, i32) {
    let Some(style) = node.primary_styles() else {
        return (0, 0);
    };
    let position = style.clone_position();
    if is_flex_or_grid {
        match position {
            Position::Static => (0, style.clone_order()),
            Position::Relative | Position::Sticky => (2, style.clone_order()),
            // Out-of-flow children are not flex/grid items: `order` does
            // not apply; tree order does.
            Position::Absolute | Position::Fixed => (2, 0),
        }
    } else {
        (
            position_to_order(position) + float_to_order(style.clone_float()),
            0,
        )
    }
}
