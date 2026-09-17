//! Construction of the paint tree (`Node::paint_children` and
//! `Node::stacking_context`) from the laid-out box tree.
//!
//! This runs once per frame after layout, when Taffy's out-of-flow pass has
//! recorded each hoisted box's containing block (`Node::oof_containing_block`).
//! Paint ownership follows DOM stacking-context ancestry, while geometry
//! (offsets, scrolling) follows the containing-block chain and is derived at
//! use-time by `Node::hoisted_child_position`.

use blitz_traits::node_id::NodeId;
use std::ops::Range;

use crate::{BaseDocument, Node};
use style::properties::generated::longhands::position::computed_value::T as Position;
use style::values::computed::Float;
use style::values::specified::box_::DisplayInside;
use taffy::Rect;

/// A child with a z_index that is hoisted up to it's containing Stacking Context for paint purposes.
///
/// The child's position relative to the stacking context root is not stored:
/// it is derived at use-time from the `containing_block()` chain (see
/// `Node::hoisted_child_position`), so it never goes stale when layout or
/// scroll offsets change elsewhere in the tree.
#[derive(Debug, Clone)]
pub struct HoistedPaintChild {
    pub node_id: NodeId,
    pub z_index: i32,
}

#[derive(Debug)]
pub struct HoistedPaintChildren {
    pub children: Vec<HoistedPaintChild>,
    /// The number of hoisted point children with negative z_index
    pub negative_z_count: u32,

    pub content_area: taffy::Rect<f32>,
}

impl HoistedPaintChildren {
    fn new() -> Self {
        Self {
            children: Vec::new(),
            negative_z_count: 0,
            content_area: taffy::Rect::ZERO,
        }
    }

    pub fn reset(&mut self) {
        self.children.clear();
        self.negative_z_count = 0;
    }

    pub fn compute_content_size(&mut self, doc: &BaseDocument, owner_id: NodeId) {
        fn child_pos(child: &HoistedPaintChild, doc: &BaseDocument, owner_id: NodeId) -> Rect<f32> {
            let node = &doc.nodes[child.node_id];
            let position = doc.nodes[owner_id].hoisted_child_position(child.node_id);
            let left = position.x + node.final_layout().location.x;
            let top = position.y + node.final_layout().location.y;
            let right = left + node.final_layout().size.width;
            let bottom = top + node.final_layout().size.height;

            taffy::Rect {
                top,
                left,
                bottom,
                right,
            }
        }

        if self.children.is_empty() {
            self.content_area = taffy::Rect::ZERO;
        } else {
            self.content_area = child_pos(&self.children[0], doc, owner_id);
            for child in self.children[1..].iter() {
                let pos = child_pos(child, doc, owner_id);
                self.content_area.left = self.content_area.left.min(pos.left);
                self.content_area.top = self.content_area.top.min(pos.top);
                self.content_area.right = self.content_area.right.max(pos.right);
                self.content_area.bottom = self.content_area.bottom.max(pos.bottom);
            }
        }
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
    /// Each node's in-flow children are placed in its `paint_children` or, when
    /// they have a `z-index`, in the nearest DOM ancestor stacking context.
    /// Out-of-flow (absolute/fixed) boxes follow the same rule when they have a
    /// `z-index`. With `z-index: auto` they bubble up the DOM tree from their
    /// parent until they reach either:
    ///
    /// - their containing block, which paints them from its `paint_children`
    ///   (in tree order among its positioned children), or
    /// - a stacking context root (opacity, filter, transform, ...), which
    ///   paints them as `z-index: 0` entries of its stacking context with a
    ///   compensating offset (see `Node::hoisted_child_position`).
    ///
    /// In incremental mode clean subtrees are skipped: the entries they
    /// contributed to an ancestor stacking context and the boxes that bubbled
    /// out of them are replayed from per-node caches.
    pub(crate) fn build_paint_tree(&mut self, root_id: NodeId) {
        let mut bubbling = Vec::new();
        self.build_paint_tree_impl(root_id, None, &mut bubbling);
    }

    fn build_paint_tree_impl(
        &mut self,
        node_id: NodeId,
        parent_stacking_context: Option<&mut HoistedPaintChildren>,
        bubbling: &mut Vec<NodeId>,
    ) {
        let incremental = self.incremental_layout;

        // Skip clean subtrees. Damage propagation bubbles descendant damage
        // into each ancestor's stored damage, so an empty damage value covers
        // the whole subtree; both a hoisted box's DOM parent and its containing
        // block are DOM ancestors of it, so any change to where it is hoisted
        // dirties every node whose paint lists it may appear in. Anonymous
        // boxes are never skipped (damage marking walks the DOM parent chain,
        // which bypasses them).
        if incremental {
            let node = &self.nodes[node_id];
            let clean = node.damage().is_some_and(|d| d.is_empty())
                && !node.has_damaged_descendants()
                && !node.is_anonymous();
            if clean {
                if let Some(parent_stacking_context) = parent_stacking_context {
                    parent_stacking_context
                        .children
                        .extend(node.sc_contribution_cache.borrow().iter().cloned());
                }
                bubbling.extend(node.oof_bubble_cache.borrow().iter().copied());
                return;
            }
        }

        let Some(display) = self.nodes[node_id].display_style() else {
            return;
        };
        let is_flex_or_grid = matches!(display.inside(), DisplayInside::Flex | DisplayInside::Grid);
        let is_stacking_context_root = parent_stacking_context.is_none();

        let mut new_stacking_context = HoistedPaintChildren::new();
        let stacking_context = &mut new_stacking_context;
        // `z-index: auto` out-of-flow boxes from this subtree whose containing
        // block is above this node, in tree order.
        let mut my_bubbling: Vec<NodeId> = Vec::new();

        let children = self.nodes[node_id].layout_children.borrow_mut().take();
        if let Some(children) = children {
            let mut paint_children = self.nodes[node_id]
                .paint_children
                .borrow_mut()
                .take()
                .unwrap_or_default();
            paint_children.clear();
            paint_children.reserve(children.len());

            for &child_id in children.iter() {
                let child = &self.nodes[child_id];
                let child_is_stacking_context_root =
                    child.is_stacking_context_root(is_flex_or_grid);

                let tail_start = my_bubbling.len();
                match child.primary_styles() {
                    None => paint_children.push(child_id),
                    Some(style) => {
                        let position = style.clone_position();
                        let z_index = style.clone_z_index().integer_or(0);
                        let is_out_of_flow = child.is_out_of_flow();
                        // Boxes which Taffy did not hoist keep their layout
                        // location relative to this node.
                        let containing_block = child
                            .oof_containing_block
                            .get()
                            .filter(|_| is_out_of_flow)
                            .unwrap_or(node_id);

                        // TODO: more complete hoisting detection
                        // z-index applies to static flex/grid items too
                        // (css-flexbox-1 §painting, css-grid-1 §z-order).
                        if z_index != 0 && (position != Position::Static || is_flex_or_grid) {
                            stacking_context.children.push(HoistedPaintChild {
                                node_id: child_id,
                                z_index,
                            })
                        } else if containing_block == node_id {
                            paint_children.push(child_id);
                        } else {
                            my_bubbling.push(child_id);
                        }
                    }
                }

                self.build_paint_tree_impl(
                    child_id,
                    match child_is_stacking_context_root {
                        true => None,
                        false => Some(stacking_context),
                    },
                    &mut my_bubbling,
                );

                // Claim the boxes bubbling out of this child (and the child
                // itself) whose containing block is this node. They are pushed
                // directly after the child so that the stable paint-order sort
                // below keeps positioned descendants in tree order.
                let mut keep = tail_start;
                for i in tail_start..my_bubbling.len() {
                    let hoisted_id = my_bubbling[i];
                    if self.nodes[hoisted_id].oof_containing_block.get() == Some(node_id) {
                        paint_children.push(hoisted_id);
                    } else {
                        my_bubbling[keep] = hoisted_id;
                        keep += 1;
                    }
                }
                my_bubbling.truncate(keep);
            }

            paint_children.sort_by(|left, right| {
                let left_node = self.nodes.get(*left).unwrap();
                let right_node = self.nodes.get(*right).unwrap();
                node_to_paint_order(left_node, is_flex_or_grid)
                    .cmp(&node_to_paint_order(right_node, is_flex_or_grid))
            });

            let node = &self.nodes[node_id];
            *node.paint_children.borrow_mut() = Some(paint_children);
            *node.layout_children.borrow_mut() = Some(children);
        }

        // Boxes whose containing block is above a stacking context root are
        // still painted inside it (opacity, filters, etc. apply to all DOM
        // descendants): they become `z-index: 0` entries of its stacking
        // context. Otherwise they keep bubbling up to their containing block.
        if is_stacking_context_root {
            stacking_context
                .children
                .extend(my_bubbling.drain(..).map(|node_id| HoistedPaintChild {
                    node_id,
                    z_index: 0,
                }));
        }

        if incremental {
            let node = &self.nodes[node_id];
            let mut cache = node.oof_bubble_cache.borrow_mut();
            cache.clear();
            cache.extend(my_bubbling.iter().copied());
        }
        bubbling.append(&mut my_bubbling);

        if let Some(parent_stacking_context) = parent_stacking_context {
            if incremental {
                let node = &self.nodes[node_id];
                let mut cache = node.sc_contribution_cache.borrow_mut();
                cache.clear();
                cache.extend(stacking_context.children.iter().cloned());
            }
            parent_stacking_context
                .children
                .extend(stacking_context.children.iter().cloned());
        } else {
            stacking_context.sort();
            stacking_context.compute_content_size(self, node_id);
            self.nodes[node_id].stacking_context = Some(Box::new(new_stacking_context));
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
