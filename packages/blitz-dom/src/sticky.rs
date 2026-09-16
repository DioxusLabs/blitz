//! `position: sticky`.
//!
//! A sticky box is laid out as if it were `position: relative`; then, once
//! layout and scroll positions are known, it is shifted so that it stays
//! within the inset rectangle of its nearest scrolling ancestor's scrollport,
//! without ever leaving its containing block (its parent's content box).
//!
//! Two phases, so that scrolling is cheap:
//! - after layout ([`BaseDocument::update_sticky_offsets`]): for every
//!   registered sticky box, everything that does not depend on scroll
//!   positions — the scroll container, the box's in-flow rect and margins,
//!   the containing-block rect, the resolved insets, the overconstraint
//!   winners — is computed once into a [`StickyConstraints`];
//! - on every scroll write and paint ([`BaseDocument::refresh_sticky_offsets`]):
//!   a handful of clamps per sticky box ([`axis_offset`]) using the current
//!   scroll offsets. No tree walk, no style access.
//!
//! The list of sticky boxes is maintained where styles are flushed to layout
//! ([`BaseDocument::note_sticky`]), kept sorted by depth so an outer sticky
//! box is always resolved before a nested one; entries of removed nodes are
//! dropped at the next update.
//!
//! The shift is stored in [`LayoutData::sticky_offset`] and applied by
//! painting, hit testing and CSSOM geometry. Covered: both axes, percentages
//! against the scrollport, the margin box kept inside the containing block,
//! nested sticky boxes, overconstrained insets resolved by writing mode and
//! direction of the containing block, transforms (the shift is applied before
//! the box's own transform), table parts (a sticky cell is constrained by the
//! table; a sticky row/row group is measured from its cells and its shift
//! handed down to them), the scrollport intersection of scrollers between the
//! box and its containing block, and `position: fixed` ancestors (a scroller
//! that never scrolls).
//!
//! [`LayoutData::sticky_offset`]: crate::node::LayoutData::sticky_offset

use blitz_traits::node_id::NodeId;
use style::computed_values::direction::T as Direction;
use style::computed_values::position::T as Position;
use style::logical_geometry::WritingModeProperty as WritingMode;
use style::values::computed::LengthPercentage;
use style::values::computed::Overflow;
use style::values::computed::length::CSSPixelLength;
use style::values::generics::position::Inset as GenericInset;

use crate::BaseDocument;

/// A rectangle in the coordinate space of some ancestor's border box.
#[derive(Clone, Copy, Debug)]
struct Rect {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

fn resolve_inset<P>(val: &GenericInset<P, LengthPercentage>, basis: f32) -> Option<f32> {
    match val {
        GenericInset::LengthPercentage(lp) => Some(lp.resolve(CSSPixelLength::new(basis)).px()),
        _ => None,
    }
}

fn scrolls(overflow: Overflow) -> bool {
    !matches!(overflow, Overflow::Visible | Overflow::Clip)
}

/// A scroll container met between the box and its containing block, whose
/// scrollport further clips the sticky view rectangle (css-position §3.5).
#[derive(Clone, Copy, Debug)]
struct ExtraScroller {
    node: NodeId,
    /// Origin of its border box in the primary container's space.
    origin_x: f32,
    origin_y: f32,
}

/// Everything about one sticky box that does not depend on scroll positions.
#[derive(Clone, Debug)]
pub struct StickyConstraints {
    /// Nearest scroll container (or fixed ancestor); `None` = the viewport.
    container: Option<NodeId>,
    /// Sticky ancestors between the box and the container, outermost first:
    /// their current shifts move this box's in-flow position.
    sticky_ancestors: Vec<NodeId>,
    /// In-flow border-box position and size in the container's space.
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    margin: taffy::Rect<f32>,
    /// Containing block (content box), same space.
    cb: Rect,
    /// Resolved insets (against the scrollport size).
    top: Option<f32>,
    bottom: Option<f32>,
    left: Option<f32>,
    right: Option<f32>,
    start_wins_x: bool,
    start_wins_y: bool,
    extra_scrollers: Vec<ExtraScroller>,
    /// A sticky row / row group: hand the shift down to the cells.
    boxless: bool,
}

/// Registry entry: depth keeps outer boxes before nested ones.
#[derive(Clone, Debug)]
pub struct StickyEntry {
    pub node: NodeId,
    depth: u32,
    constraints: Option<StickyConstraints>,
}

impl BaseDocument {
    /// Registers or unregisters a node as `position: sticky`; called wherever
    /// a node's styles are flushed to layout, so the list follows style
    /// changes without any tree walk of its own.
    pub(crate) fn note_sticky(&mut self, node_id: NodeId, is_sticky: bool) {
        // The flag mirrors registration, so the common case (no change) costs
        // one bit test and no search.
        let registered = self.nodes[node_id]
            .flags
            .contains(crate::node::NodeFlags::IS_STICKY);
        if registered == is_sticky {
            return;
        }
        self.nodes[node_id]
            .flags
            .set(crate::node::NodeFlags::IS_STICKY, is_sticky);
        let index = self.sticky_nodes.iter().position(|e| e.node == node_id);
        match (index, is_sticky) {
            (Some(_), true) | (None, false) => {}
            (Some(i), false) => {
                self.sticky_nodes.remove(i);
            }
            (None, true) => {
                let mut depth = 0;
                let mut cur = self.nodes[node_id].parent;
                while let Some(p) = cur {
                    depth += 1;
                    cur = self.nodes[p].parent;
                }
                let at = self.sticky_nodes.partition_point(|e| e.depth <= depth);
                self.sticky_nodes.insert(
                    at,
                    StickyEntry {
                        node: node_id,
                        depth,
                        constraints: None,
                    },
                );
            }
        }
    }

    /// After layout: recomputes the layout-invariant constraints of every
    /// registered sticky box, drops entries of nodes that left the document,
    /// then applies the current scroll positions.
    pub fn update_sticky_offsets(&mut self) {
        if self.sticky_nodes.is_empty() {
            return;
        }
        let root = self.root_node_id;
        let viewport = self.viewport();
        let scale = viewport.scale_f64() as f32;
        let viewport_size = (
            viewport.window_size.0 as f32 / scale,
            viewport.window_size.1 as f32 / scale,
        );
        let mut entries = std::mem::take(&mut self.sticky_nodes);
        // Drop nodes that left the document or have no box (`display: none`),
        // clearing their flag so a node moved back in (keyed lists) registers
        // again: the flag must mirror registration exactly.
        let mut dropped = Vec::new();
        entries.retain(|e| {
            let keep = self.nodes.get(e.node).is_some_and(|n| {
                n.flags.is_in_document()
                    && n.element_data().is_some()
                    && n.display_style().is_some()
            });
            if !keep {
                dropped.push(e.node);
            }
            keep
        });
        for id in dropped {
            if let Some(n) = self.nodes.get_mut(id) {
                n.flags.set(crate::node::NodeFlags::IS_STICKY, false);
            }
        }
        for entry in entries.iter_mut() {
            entry.constraints = self.constraints_for(entry.node, root, viewport_size);
        }
        self.sticky_nodes = entries;
        self.refresh_sticky_offsets();
    }

    /// Applies the current scroll positions: a few clamps per sticky box.
    pub fn refresh_sticky_offsets(&mut self) {
        if self.sticky_nodes.is_empty() {
            return;
        }
        let viewport = self.viewport();
        let scale = viewport.scale_f64() as f32;
        let viewport_size = (
            viewport.window_size.0 as f32 / scale,
            viewport.window_size.1 as f32 / scale,
        );
        let vs = self.viewport_scroll();
        let viewport_scroll = (vs.x as f32, vs.y as f32);

        let entries = std::mem::take(&mut self.sticky_nodes);
        // Pass 1: clear (shifts handed down by boxless table parts accumulate).
        for entry in &entries {
            if let Some(n) = self.nodes.get_mut(entry.node) {
                n.layout_data_mut().sticky_offset = crate::Point { x: 0.0, y: 0.0 };
            }
            if entry.constraints.as_ref().is_some_and(|c| c.boxless) {
                self.hand_down_shift(entry.node, f32::NAN, f32::NAN);
            }
        }
        // Pass 2: outer boxes first (sorted by depth), so a nested box reads
        // its ancestors' shifts already resolved.
        for entry in &entries {
            let Some(c) = entry.constraints.as_ref() else {
                continue;
            };
            let (x, y) = self.shift_for(c, viewport_size, viewport_scroll);
            let ld = self.nodes[entry.node].layout_data_mut();
            ld.sticky_offset.x += x;
            ld.sticky_offset.y += y;
            if c.boxless {
                self.hand_down_shift(entry.node, x, y);
            }
        }
        self.sticky_nodes = entries;
    }

    /// The layout-invariant part: where the box and its containing block sit
    /// relative to the scroll container, and what the insets resolve to.
    fn constraints_for(
        &self,
        id: NodeId,
        root: NodeId,
        viewport_size: (f32, f32),
    ) -> Option<StickyConstraints> {
        let node = &self.nodes[id];
        let styles = node.primary_styles()?;
        let parent_id = node.parent?;
        let layout = node.final_layout();
        let boxless = is_boxless_table_part(node);
        let (own_x, own_y, width, height, margin) = if !boxless {
            (
                layout.location.x,
                layout.location.y,
                layout.size.width,
                layout.size.height,
                layout.margin,
            )
        } else {
            let (x0, y0, x1, y1) = self.cells_extent(id)?;
            (x0, y0, x1 - x0, y1 - y0, taffy::Rect::zero())
        };

        // Walk up the *layout* parent chain (locations are relative to it: an
        // inline-level box's layout parent is the inline root, not its span)
        // to the nearest scroll container (or fixed ancestor), noting sticky
        // ancestors on the way; positions stay in-flow here — the ancestors'
        // shifts are applied at refresh time.
        let mut x = own_x;
        let mut y = own_y;
        let mut container: Option<NodeId> = None;
        let mut sticky_ancestors = Vec::new();
        let mut cur = self.layout_ancestor(id);
        while let Some(a) = cur {
            if a == root {
                break;
            }
            let anc = &self.nodes[a];
            if let Some(s) = anc.primary_styles() {
                let b = s.get_box();
                let position = s.clone_position();
                if scrolls(b.overflow_x) || scrolls(b.overflow_y) || position == Position::Fixed {
                    container = Some(a);
                    break;
                }
                if position == Position::Sticky {
                    sticky_ancestors.push(a);
                }
            }
            let l = anc.final_layout();
            x += l.location.x;
            y += l.location.y;
            cur = self.layout_ancestor(a);
        }
        sticky_ancestors.reverse();

        // Scrollport size, for percentage insets.
        let (port_w, port_h) = match container {
            Some(c) => {
                let l = self.nodes[c].final_layout();
                (
                    l.size.width - l.border.left - l.border.right,
                    l.size.height - l.border.top - l.border.bottom,
                )
            }
            None => viewport_size,
        };

        // Containing block: the content box of the nearest layout ancestor
        // that has a box (for a cell in a row: the table).
        let mut cb_id = self.layout_ancestor(id).unwrap_or(parent_id);
        let mut parent_x = x - own_x;
        let mut parent_y = y - own_y;
        while cb_id != root && is_boxless_table_part(&self.nodes[cb_id]) {
            let Some(up) = self.layout_ancestor(cb_id) else {
                break;
            };
            let l = self.nodes[cb_id].final_layout();
            parent_x -= l.location.x;
            parent_y -= l.location.y;
            cb_id = up;
        }
        let parent = &self.nodes[cb_id];
        let pl = parent.final_layout();
        let mut cb = Rect {
            x0: parent_x + pl.border.left + pl.padding.left,
            y0: parent_y + pl.border.top + pl.padding.top,
            x1: parent_x + pl.size.width - pl.border.right - pl.padding.right,
            y1: parent_y + pl.size.height - pl.border.bottom - pl.padding.bottom,
        };
        if container == Some(cb_id) {
            // The scroll container's content box is the whole scrollable area.
            cb.x1 = cb
                .x1
                .max(parent_x + pl.border.left + parent.scroll_width() - pl.padding.right);
            cb.y1 = cb
                .y1
                .max(parent_y + pl.border.top + parent.scroll_height() - pl.padding.bottom);
        }

        // Scrollers strictly between the box and its containing block (only
        // when the container sits below the containing block): their
        // scrollports further clip the view rectangle at refresh time.
        let mut extra_scrollers = Vec::new();
        if let Some(first) = container {
            let below_cb = {
                let mut cur = self.layout_ancestor(first);
                let mut found = false;
                while let Some(a) = cur {
                    if a == cb_id {
                        found = true;
                        break;
                    }
                    cur = self.layout_ancestor(a);
                }
                found
            };
            if below_cb {
                let mut origin_x = 0.0f32;
                let mut origin_y = 0.0f32;
                let mut cur = Some(first);
                while let Some(c) = cur {
                    if c == cb_id || c == root {
                        break;
                    }
                    let cn = &self.nodes[c];
                    let l = cn.final_layout();
                    origin_x -= l.location.x;
                    origin_y -= l.location.y;
                    let Some(up) = self.layout_ancestor(c) else {
                        break;
                    };
                    if up == root {
                        break;
                    }
                    if let Some(s) = self.nodes[up].primary_styles() {
                        let b = s.get_box();
                        if scrolls(b.overflow_x) || scrolls(b.overflow_y) {
                            extra_scrollers.push(ExtraScroller {
                                node: up,
                                origin_x,
                                origin_y,
                            });
                        }
                    }
                    cur = Some(up);
                }
            }
        }

        let pos = styles.get_position();
        let (writing_mode, direction) = match parent.primary_styles() {
            Some(s) => (
                s.get_inherited_box().writing_mode,
                s.get_inherited_box().direction,
            ),
            None => (
                styles.get_inherited_box().writing_mode,
                styles.get_inherited_box().direction,
            ),
        };
        let (start_wins_x, start_wins_y) = overconstraint_winners(writing_mode, direction);
        Some(StickyConstraints {
            container,
            sticky_ancestors,
            x,
            y,
            width,
            height,
            margin,
            cb,
            top: resolve_inset(&pos.top, port_h),
            bottom: resolve_inset(&pos.bottom, port_h),
            left: resolve_inset(&pos.left, port_w),
            right: resolve_inset(&pos.right, port_w),
            start_wins_x,
            start_wins_y,
            extra_scrollers,
            boxless,
        })
    }

    /// The node whose coordinate space `final_layout().location` is relative
    /// to: the layout parent, falling back to the DOM parent for nodes that
    /// are not in the layout tree (boxless table parts).
    fn layout_ancestor(&self, id: NodeId) -> Option<NodeId> {
        let node = &self.nodes[id];
        node.layout_parent.get().or(node.parent)
    }

    /// The scroll-dependent part: the shift for the current scroll offsets.
    fn shift_for(
        &self,
        c: &StickyConstraints,
        viewport_size: (f32, f32),
        viewport_scroll: (f32, f32),
    ) -> (f32, f32) {
        // Ancestors' shifts (already resolved, outer first) move the in-flow
        // position and the containing block alike.
        let mut dx = 0.0f32;
        let mut dy = 0.0f32;
        for a in &c.sticky_ancestors {
            if let Some(n) = self.nodes.get(*a) {
                let s = n.sticky_offset();
                dx += s.x;
                dy += s.y;
            }
        }
        let mut scrollport = match c.container {
            Some(id) => scrollport_of(&self.nodes[id], 0.0, 0.0),
            None => Rect {
                x0: viewport_scroll.0,
                y0: viewport_scroll.1,
                x1: viewport_scroll.0 + viewport_size.0,
                y1: viewport_scroll.1 + viewport_size.1,
            },
        };
        for extra in &c.extra_scrollers {
            if let Some(n) = self.nodes.get(extra.node) {
                scrollport = scrollport.intersect(scrollport_of(n, extra.origin_x, extra.origin_y));
            }
        }
        let cb = Rect {
            x0: c.cb.x0 + dx,
            y0: c.cb.y0 + dy,
            x1: c.cb.x1 + dx,
            y1: c.cb.y1 + dy,
        };
        (
            axis_offset(
                Axis {
                    pos: c.x + dx,
                    size: c.width,
                    margin_start: c.margin.left,
                    margin_end: c.margin.right,
                    start_inset: c.left,
                    end_inset: c.right,
                    start_wins: c.start_wins_x,
                },
                scrollport.x0,
                scrollport.x1,
                cb.x0,
                cb.x1,
            ),
            axis_offset(
                Axis {
                    pos: c.y + dy,
                    size: c.height,
                    margin_start: c.margin.top,
                    margin_end: c.margin.bottom,
                    start_inset: c.top,
                    end_inset: c.bottom,
                    start_wins: c.start_wins_y,
                },
                scrollport.y0,
                scrollport.y1,
                cb.y0,
                cb.y1,
            ),
        )
    }

    /// Border-box extent (x0, y0, x1, y1) of the cells under a boxless table
    /// part, in its table's coordinate space.
    fn cells_extent(&self, id: NodeId) -> Option<(f32, f32, f32, f32)> {
        let mut extent: Option<(f32, f32, f32, f32)> = None;
        let mut stack: Vec<NodeId> = self.nodes[id].children.iter().copied().collect();
        while let Some(c) = stack.pop() {
            let node = &self.nodes[c];
            if node.element_data().is_none() {
                continue;
            }
            if !is_boxless_table_part(node) {
                let l = node.final_layout();
                let (x0, y0) = (l.location.x, l.location.y);
                let (x1, y1) = (x0 + l.size.width, y0 + l.size.height);
                extent = Some(match extent {
                    None => (x0, y0, x1, y1),
                    Some(e) => (e.0.min(x0), e.1.min(y0), e.2.max(x1), e.3.max(y1)),
                });
            } else {
                stack.extend(node.children.iter().copied());
            }
        }
        extent
    }

    /// Adds a boxless ancestor's shift to the cells that paint it (`NaN`
    /// clears). Iterates by index to avoid allocating per child.
    fn hand_down_shift(&mut self, id: NodeId, dx: f32, dy: f32) {
        let count = self.nodes[id].children.len();
        for i in 0..count {
            let c = self.nodes[id].children[i];
            if self.nodes[c].element_data().is_none() {
                continue;
            }
            if !is_boxless_table_part(&self.nodes[c]) {
                let ld = self.nodes[c].layout_data_mut();
                if dx.is_nan() {
                    ld.sticky_offset = crate::Point { x: 0.0, y: 0.0 };
                } else {
                    ld.sticky_offset.x += dx;
                    ld.sticky_offset.y += dy;
                }
            } else {
                self.hand_down_shift(c, dx, dy);
            }
        }
    }
}

/// Table parts that get no box of their own in Blitz: their cells are laid
/// out directly in the table's grid (`layout/table.rs`).
fn is_boxless_table_part(node: &crate::node::Node) -> bool {
    use style::values::specified::box_::DisplayInside;
    node.primary_styles().is_some_and(|s| {
        matches!(
            s.clone_display().inside(),
            DisplayInside::TableRow
                | DisplayInside::TableRowGroup
                | DisplayInside::TableHeaderGroup
                | DisplayInside::TableFooterGroup
                | DisplayInside::TableColumn
                | DisplayInside::TableColumnGroup
        )
    })
}

/// Scrollport (padding box, offset by the current scroll) of a scroll
/// container whose border-box origin is at (`ox`, `oy`) in the target space.
fn scrollport_of(node: &crate::node::Node, ox: f32, oy: f32) -> Rect {
    let l = node.final_layout();
    let so = node.scroll_offset();
    let x0 = ox + l.border.left + so.x as f32;
    let y0 = oy + l.border.top + so.y as f32;
    Rect {
        x0,
        y0,
        x1: x0 + (l.size.width - l.border.left - l.border.right),
        y1: y0 + (l.size.height - l.border.top - l.border.bottom),
    }
}

impl Rect {
    fn intersect(self, other: Rect) -> Rect {
        Rect {
            x0: self.x0.max(other.x0),
            y0: self.y0.max(other.y0),
            x1: self.x1.min(other.x1).max(self.x0.max(other.x0)),
            y1: self.y1.min(other.y1).max(self.y0.max(other.y0)),
        }
    }
}

/// Which inset wins when both are given and cannot both be honoured: the
/// block-start and inline-start insets, per writing mode and direction.
/// Returns (`left` wins on x, `top` wins on y).
fn overconstraint_winners(writing_mode: WritingMode, direction: Direction) -> (bool, bool) {
    let ltr = direction == Direction::Ltr;
    match writing_mode {
        WritingMode::HorizontalTb => (ltr, true),
        // Vertical: the block axis is horizontal (block-start = right for *-rl),
        // the inline axis is vertical (inline-start = top when ltr).
        WritingMode::VerticalRl => (false, ltr),
        WritingMode::VerticalLr => (true, ltr),
        // `sideways-*` only exists in Gecko builds of Stylo; treat any other
        // mode like its vertical counterpart.
        #[allow(unreachable_patterns)]
        _ => (false, ltr),
    }
}

/// One axis of the sticky computation.
#[derive(Clone, Copy, Debug)]
struct Axis {
    /// Border-box start position of the box in the scroller's space.
    pos: f32,
    /// Border-box size on this axis.
    size: f32,
    margin_start: f32,
    margin_end: f32,
    start_inset: Option<f32>,
    end_inset: Option<f32>,
    /// When both insets are given and conflict, honour the start one.
    start_wins: bool,
}

/// Positions the box inside the inset edges of the scrollport, then keeps its
/// margin box inside its containing block. Returns the shift to apply.
///
/// Spec order (CSS Positioned Layout 3, §3.5): the end inset is applied first,
/// then the start inset, so when the box is taller than the space between
/// them the start inset wins (or the reverse in modes where the end wins).
fn axis_offset(a: Axis, port_start: f32, port_end: f32, cb_start: f32, cb_end: f32) -> f32 {
    let mut position = a.pos;
    let apply_start = |position: &mut f32| {
        if let Some(inset) = a.start_inset {
            let limit = port_start + inset;
            if *position < limit {
                *position = limit;
            }
        }
    };
    let apply_end = |position: &mut f32| {
        if let Some(inset) = a.end_inset {
            let limit = port_end - inset - a.size;
            if *position > limit {
                *position = limit;
            }
        }
    };
    if a.start_wins {
        apply_end(&mut position);
        apply_start(&mut position);
    } else {
        apply_start(&mut position);
        apply_end(&mut position);
    }

    // Never leave the containing block (margin box), and never move a box that
    // would only be moved by the containing-block clamp itself.
    let min = cb_start + a.margin_start;
    let max = cb_end - a.margin_end - a.size;
    if position > a.pos {
        position = position.min(max.max(a.pos));
    } else if position < a.pos {
        position = position.max(min.min(a.pos));
    }
    position - a.pos
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axis(pos: f32, size: f32, start: Option<f32>, end: Option<f32>) -> Axis {
        Axis {
            pos,
            size,
            margin_start: 0.0,
            margin_end: 0.0,
            start_inset: start,
            end_inset: end,
            start_wins: true,
        }
    }

    #[test]
    fn sticks_to_top_and_stops_at_containing_block_end() {
        // Box at y=100, 20px tall, containing block 0..=400, viewport scrolled to 150 with top: 10.
        assert_eq!(
            axis_offset(
                axis(100.0, 20.0, Some(10.0), None),
                150.0,
                750.0,
                0.0,
                400.0
            ),
            60.0
        );
        // Scrolled past the containing block: the box stops at cb_end - size.
        assert_eq!(
            axis_offset(
                axis(100.0, 20.0, Some(10.0), None),
                600.0,
                1200.0,
                0.0,
                400.0
            ),
            280.0
        );
        // Not yet reached: no shift.
        assert_eq!(
            axis_offset(axis(100.0, 20.0, Some(10.0), None), 0.0, 600.0, 0.0, 400.0),
            0.0
        );
    }

    #[test]
    fn sticks_to_bottom() {
        // Box at y=900 in a 600px viewport at scroll 0 with bottom: 0 → pulled up to 580.
        assert_eq!(
            axis_offset(axis(900.0, 20.0, None, Some(0.0)), 0.0, 600.0, 0.0, 1000.0),
            -320.0
        );
        // Cannot move above its containing block start.
        assert_eq!(
            axis_offset(
                axis(900.0, 20.0, None, Some(0.0)),
                0.0,
                600.0,
                700.0,
                1000.0
            ),
            -200.0
        );
    }

    #[test]
    fn margins_stay_inside_the_containing_block() {
        let mut a = axis(100.0, 20.0, Some(0.0), None);
        a.margin_end = 10.0;
        // Without the margin the box could reach y=380; the 10px margin stops it at 370.
        assert_eq!(axis_offset(a, 600.0, 1200.0, 0.0, 400.0), 270.0);
    }

    #[test]
    fn overconstrained_insets_follow_the_winner() {
        // A 20px box with top: 0 and bottom: 0 in a 10px-tall scrollport: both pull.
        let mut a = axis(50.0, 20.0, Some(0.0), Some(0.0));
        assert_eq!(axis_offset(a, 100.0, 110.0, 0.0, 1000.0), 50.0); // top wins → y = 100
        a.start_wins = false;
        assert_eq!(axis_offset(a, 100.0, 110.0, 0.0, 1000.0), 40.0); // bottom wins → y = 90
    }

    #[test]
    fn writing_mode_and_direction_pick_the_winner() {
        assert_eq!(
            overconstraint_winners(WritingMode::HorizontalTb, Direction::Ltr),
            (true, true)
        );
        assert_eq!(
            overconstraint_winners(WritingMode::HorizontalTb, Direction::Rtl),
            (false, true)
        );
        assert_eq!(
            overconstraint_winners(WritingMode::VerticalRl, Direction::Ltr),
            (false, true)
        );
        assert_eq!(
            overconstraint_winners(WritingMode::VerticalLr, Direction::Rtl),
            (true, false)
        );
    }

    #[test]
    fn taller_than_the_space_between_insets_follows_the_winner() {
        // WPT top-and-bottom-overconstrained: 200px box, scroller 100px, top: 20, bottom: 0,
        // in-flow at y=200 in a 600px containing block.
        let a = |start_wins| Axis {
            pos: 200.0,
            size: 200.0,
            margin_start: 0.0,
            margin_end: 0.0,
            start_inset: Some(20.0),
            end_inset: Some(0.0),
            start_wins,
        };
        assert_eq!(200.0 + axis_offset(a(true), 0.0, 100.0, 0.0, 600.0), 20.0);
        assert_eq!(
            200.0 + axis_offset(a(true), 160.0, 260.0, 0.0, 600.0),
            180.0
        );
        assert_eq!(
            200.0 + axis_offset(a(true), 220.0, 320.0, 0.0, 600.0),
            240.0
        );
        assert_eq!(
            200.0 + axis_offset(a(true), 500.0, 600.0, 0.0, 600.0),
            400.0
        );
    }

    #[test]
    fn auto_insets_do_nothing() {
        assert_eq!(
            axis_offset(axis(100.0, 20.0, None, None), 500.0, 1100.0, 0.0, 400.0),
            0.0
        );
    }
}
