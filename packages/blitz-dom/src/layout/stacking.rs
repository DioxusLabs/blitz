//! Stacking-context membership resolution.
//!
//! After layout, a post-layout pass walks the structural (layout) tree and
//! derives paint membership:
//!
//! - `paint_children` on each node retains only its in-flow (non-stacked)
//!   children, sorted so floats paint above other in-flow content and
//!   flex/grid `order` is honoured.
//! - Every *stacked* box (positioned boxes, flex/grid items with a z-index,
//!   and boxes which establish a real stacking context for any other reason)
//!   is recorded as a [`StackingEntry`] in the entry list of the nearest
//!   enclosing *real* stacking context.
//!
//! A positioned box with `z-index: auto` is a *stacking container*: it gets
//! its own entry (painted in tree order among the zero/auto entries), but is
//! not atomic — its own stacked descendants continue to collect into the same
//! enclosing real stacking context. A real stacking context is atomic: it
//! owns its stacked descendants in its own entry list.
//!
//! Entries carry no baked geometry. Paint and hit-testing resolve an entry's
//! offset from its stacking-context root on demand by walking the geometry
//! (containing-block) parent chain — see [`Node::stacked_entry_offset`].

use blitz_traits::node_id::NodeId;
use style::selector_parser::RestyleDamage;
use style::values::specified::box_::DisplayInside;
use thin_vec::ThinVec;

use crate::BaseDocument;
use crate::node::Node;

/// A stacked box recorded in the entry list of its nearest enclosing real
/// stacking context.
#[derive(Debug, Clone, Copy)]
pub struct StackingEntry {
    pub node_id: NodeId,
    /// The used z-index: the integer value when z-index applies to the box,
    /// otherwise 0 (`z-index: auto` containers and non-positioned boxes that
    /// establish a stacking context, e.g. via `opacity`).
    pub z_index: i32,
}

/// The derived paint membership of a real stacking context: all stacked
/// descendant boxes up to (but not into) descendant real stacking contexts,
/// sorted by z-index (stable, so equal-z entries keep tree order).
#[derive(Debug, Default)]
pub struct StackingContext {
    pub entries: Vec<StackingEntry>,
    /// The number of entries with a negative z-index.
    pub negative_z_count: usize,
}

impl StackingContext {
    fn finish(mut entries: Vec<StackingEntry>) -> Self {
        entries.sort_by_key(|e| e.z_index);
        let negative_z_count = entries.iter().take_while(|e| e.z_index < 0).count();
        Self {
            entries,
            negative_z_count,
        }
    }

    /// Entries with a negative z-index, in paint order.
    pub fn negative_entries(&self) -> &[StackingEntry] {
        &self.entries[..self.negative_z_count]
    }

    /// Entries with a zero/auto or positive z-index, in paint order.
    pub fn non_negative_entries(&self) -> &[StackingEntry] {
        &self.entries[self.negative_z_count..]
    }
}

/// The stacking-relevant damage bit (`REBUILD_STACKING_CONTEXT` without the
/// `REPAINT` bit it contains).
pub(crate) const STACKING_DAMAGE: RestyleDamage = RestyleDamage::from_bits_retain(0b_0010);

impl BaseDocument {
    /// Rebuild stacking-context membership and `paint_children` for the whole
    /// tree. Runs post-layout; gated by stacking damage in the caller.
    pub(crate) fn resolve_stacking_contexts(&mut self, root_node_id: NodeId) {
        let mut entries = Vec::new();
        self.collect_stacked_children(root_node_id, &mut entries);
        self.nodes[root_node_id].stacking_context = Some(Box::new(StackingContext::finish(
            std::mem::take(&mut entries),
        )));
    }

    /// Classify the children of `node_id`, rebuilding its `paint_children`
    /// (in-flow children only) and recording stacked children as entries of
    /// the current enclosing real stacking context (`entries`).
    fn collect_stacked_children(&mut self, node_id: NodeId, entries: &mut Vec<StackingEntry>) {
        let Some(display) = self.nodes[node_id].display_style() else {
            return;
        };
        let is_flex_or_grid = matches!(display.inside(), DisplayInside::Flex | DisplayInside::Grid);

        let children = self.nodes[node_id].layout_children.borrow_mut().take();
        let Some(children) = children else {
            return;
        };

        // Rebuild paint_children: in-flow (non-stacked) children only, with
        // floats painting above other in-flow content (CSS 2.1 Appendix E
        // step 4 vs step 3) and flex/grid `order` honoured.
        {
            let mut paint_children = self.nodes[node_id].paint_children.borrow_mut();
            let paint_children = paint_children.get_or_insert_with(ThinVec::new);
            paint_children.clear();
            for &child_id in children.iter() {
                if !self.nodes[child_id].is_stacked(is_flex_or_grid) {
                    paint_children.push(child_id);
                }
            }
            paint_children.sort_by_key(|&child_id| {
                in_flow_paint_order(&self.nodes[child_id], is_flex_or_grid)
            });
        }

        for &child_id in children.iter() {
            let child = &self.nodes[child_id];
            if child.is_stacked(is_flex_or_grid) {
                let is_context = child.is_stacking_context_root(is_flex_or_grid);
                // z-index only applies to positioned boxes and flex/grid items;
                // other stacking-context roots (e.g. opacity) stack at 0.
                let z_index =
                    if child.taffy_position() != taffy::Position::Static || is_flex_or_grid {
                        child.z_index()
                    } else {
                        0
                    };
                entries.push(StackingEntry {
                    node_id: child_id,
                    z_index,
                });
                if is_context {
                    // Atomic: the child owns its stacked descendants.
                    let mut inner = Vec::new();
                    self.collect_stacked_children(child_id, &mut inner);
                    self.nodes[child_id].stacking_context =
                        Some(Box::new(StackingContext::finish(inner)));
                } else {
                    // Stacking container: descendants keep collecting into the
                    // current context.
                    self.nodes[child_id].stacking_context = None;
                    self.collect_stacked_children(child_id, entries);
                }
            } else {
                self.nodes[child_id].stacking_context = None;
                self.collect_stacked_children(child_id, entries);
            }
        }

        // Put children back
        *self.nodes[node_id].layout_children.borrow_mut() = Some(children);
    }
}

/// Paint sort key for in-flow children: floats above other in-flow content
/// (CSS 2.1 Appendix E step 4 vs step 3); within a level the stable sort
/// preserves document order (order-modified document order for flex/grid
/// items, where `float` does not apply).
#[inline(always)]
fn in_flow_paint_order(node: &Node, is_flex_or_grid: bool) -> (i32, i32) {
    use style::values::computed::Float;
    let Some(style) = node.primary_styles() else {
        return (0, 0);
    };
    if is_flex_or_grid {
        (0, style.clone_order())
    } else {
        ((style.clone_float() != Float::None) as i32, 0)
    }
}
