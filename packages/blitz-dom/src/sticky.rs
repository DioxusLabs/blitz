//! `position: sticky`.
//!
//! A sticky box is laid out as if it were `position: relative`; then, once
//! layout and scroll positions are known, it is shifted so that it stays
//! within the inset rectangle of its nearest scrolling ancestor's scrollport,
//! without ever leaving its containing block (its parent's content box).
//!
//! The resolved shift is stored in [`LayoutData::sticky_offset`] and applied
//! by painting and hit testing, so layout itself is untouched and the offset
//! can be refreshed cheaply after every scroll.
//!
//! Covered: both axes, percentages against the scrollport, the margin box
//! kept inside the containing block, nested sticky boxes (an inner box sees
//! its outer box already shifted), overconstrained insets resolved by writing
//! mode and direction, transforms (the shift is applied before the box's
//! own transform, as the spec orders), and table parts: Blitz lays cells out
//! directly in the table's grid, so rows and row groups have no box of their
//! own. A sticky cell is therefore constrained by the table, and a sticky
//! row/row group is measured from its cells and its shift handed down to them.
//!
//! [`LayoutData::sticky_offset`]: crate::node::LayoutData::sticky_offset

use blitz_traits::node_id::NodeId;
use style::computed_values::direction::T as Direction;
use style::computed_values::position::T as Position;
use style::logical_geometry::WritingModeProperty as WritingMode;
use style::values::computed::Overflow;
use style::values::computed::length::CSSPixelLength;
use style::values::computed::LengthPercentage;
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

impl BaseDocument {
    /// Recomputes the sticky offset of every `position: sticky` box.
    ///
    /// Called at the end of style/layout resolution and before each paint,
    /// which also covers scrolling (a scroll always requests a repaint).
    pub fn update_sticky_offsets(&mut self) {
        let root = self.root_node_id;
        let viewport = self.viewport();
        let scale = viewport.scale_f64() as f32;
        let viewport_size = (
            viewport.window_size.0 as f32 / scale,
            viewport.window_size.1 as f32 / scale,
        );
        let viewport_scroll = self.viewport_scroll();
        let viewport_scroll = (viewport_scroll.x as f32, viewport_scroll.y as f32);

        // Pass 1: clear (shifts handed down by boxless ancestors accumulate).
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let node = &self.nodes[id];
            stack.extend(node.children.iter().copied());
            if node.element_data().is_some() {
                self.nodes[id].layout_data_mut().sticky_offset = crate::Point { x: 0.0, y: 0.0 };
            }
        }
        // Pass 2: parents are visited before children, so a nested sticky box
        // reads its ancestors' shifts already resolved.
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let node = &self.nodes[id];
            stack.extend(node.children.iter().rev().copied());
            let is_sticky = node
                .primary_styles()
                .is_some_and(|s| s.clone_position() == Position::Sticky);
            if is_sticky {
                let (x, y) = self.sticky_offset_for(id, root, viewport_size, viewport_scroll);
                let ld = self.nodes[id].layout_data_mut();
                ld.sticky_offset.x += x;
                ld.sticky_offset.y += y;
                if !has_box(&self.nodes[id]) {
                    // A sticky row / row group: its cells paint the shift.
                    self.hand_down_shift(id, x, y);
                }
            }
        }
    }

    /// Computes the (dx, dy) shift for one sticky box, in CSS pixels.
    fn sticky_offset_for(
        &self,
        id: NodeId,
        root: NodeId,
        viewport_size: (f32, f32),
        viewport_scroll: (f32, f32),
    ) -> (f32, f32) {
        let node = &self.nodes[id];
        let Some(styles) = node.primary_styles() else {
            return (0.0, 0.0);
        };
        let Some(parent_id) = node.parent else {
            return (0.0, 0.0);
        };
        let layout = node.final_layout();
        // Boxless table parts are measured from the cells they contain.
        let (own_x, own_y, width, height, margin) = if has_box(node) {
            (layout.location.x, layout.location.y, layout.size.width, layout.size.height, layout.margin)
        } else {
            match self.cells_extent(id) {
                Some((x0, y0, x1, y1)) => (x0, y0, x1 - x0, y1 - y0, taffy::Rect::zero()),
                None => return (0.0, 0.0),
            }
        };

        // Position of this box relative to the border box of the ancestor
        // whose coordinate space we end up in (the scroll container, or the
        // document when the viewport scrolls it).
        let mut x = own_x;
        let mut y = own_y;
        let mut container: Option<NodeId> = None;
        let mut cur = node.parent;
        while let Some(a) = cur {
            if a == root {
                break;
            }
            let anc = &self.nodes[a];
            if let Some(s) = anc.primary_styles() {
                let b = s.get_box();
                if scrolls(b.overflow_x) || scrolls(b.overflow_y) {
                    container = Some(a);
                    break;
                }
            }
            let l = anc.final_layout();
            let shift = anc.sticky_offset();
            x += l.location.x + shift.x;
            y += l.location.y + shift.y;
            cur = anc.parent;
        }

        // The scrollport (padding box of the scroll container, or the
        // viewport), expressed in the same coordinate space as (x, y).
        let scrollport = match container {
            Some(c) => {
                let cn = &self.nodes[c];
                let l = cn.final_layout();
                let so = cn.scroll_offset();
                Rect {
                    x0: l.border.left + so.x as f32,
                    y0: l.border.top + so.y as f32,
                    x1: l.border.left + so.x as f32 + (l.size.width - l.border.left - l.border.right),
                    y1: l.border.top + so.y as f32 + (l.size.height - l.border.top - l.border.bottom),
                }
            }
            None => Rect {
                x0: viewport_scroll.0,
                y0: viewport_scroll.1,
                x1: viewport_scroll.0 + viewport_size.0,
                y1: viewport_scroll.1 + viewport_size.1,
            },
        };

        // Containing block: the content box of the nearest ancestor that has a
        // box (for a cell in a row: the table), in the same space.
        let mut cb_id = parent_id;
        let mut parent_x = x - own_x;
        let mut parent_y = y - own_y;
        while cb_id != root && !has_box(&self.nodes[cb_id]) {
            let Some(up) = self.nodes[cb_id].parent else { break };
            let l = self.nodes[cb_id].final_layout();
            let shift = self.nodes[cb_id].sticky_offset();
            parent_x -= l.location.x + shift.x;
            parent_y -= l.location.y + shift.y;
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
        // When the containing block is the scroll container itself, its content
        // box is the whole scrollable area, not just the visible box.
        if container == Some(cb_id) {
            // `scroll_width/height`: padding box or the scrollable overflow, whichever is larger.
            cb.x1 = cb.x1.max(parent_x + pl.border.left + parent.scroll_width() - pl.padding.right);
            cb.y1 = cb.y1.max(parent_y + pl.border.top + parent.scroll_height() - pl.padding.bottom);
        }

        let pos = styles.get_position();
        let top = resolve_inset(&pos.top, scrollport.y1 - scrollport.y0);
        let bottom = resolve_inset(&pos.bottom, scrollport.y1 - scrollport.y0);
        let left = resolve_inset(&pos.left, scrollport.x1 - scrollport.x0);
        let right = resolve_inset(&pos.right, scrollport.x1 - scrollport.x0);


        // The margin box is what must stay inside the containing block.
        let m = margin;
        // Overconstraint is resolved in the containing block's writing mode and
        // direction (CSS 2.1 §9.4.3 / Position 3), not the sticky box's own.
        let (writing_mode, direction) = match parent.primary_styles() {
            Some(cb) => (cb.get_inherited_box().writing_mode, cb.get_inherited_box().direction),
            None => (styles.get_inherited_box().writing_mode, styles.get_inherited_box().direction),
        };
        let (start_wins_x, start_wins_y) = overconstraint_winners(writing_mode, direction);
        (
            axis_offset(
                Axis { pos: x, size: width, margin_start: m.left, margin_end: m.right, start_inset: left, end_inset: right, start_wins: start_wins_x },
                scrollport.x0,
                scrollport.x1,
                cb.x0,
                cb.x1,
            ),
            axis_offset(
                Axis { pos: y, size: height, margin_start: m.top, margin_end: m.bottom, start_inset: top, end_inset: bottom, start_wins: start_wins_y },
                scrollport.y0,
                scrollport.y1,
                cb.y0,
                cb.y1,
            ),
        )
    }
}

/// Whether layout gave this node a box of its own. Table rows and row groups
/// don't get one: their cells are laid out in the table's grid.
fn has_box(node: &crate::node::Node) -> bool {
    let l = node.final_layout();
    l.size.width > 0.0 || l.size.height > 0.0
}

impl BaseDocument {
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
            if has_box(node) {
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

    /// Adds a boxless ancestor's shift to the cells that paint it.
    fn hand_down_shift(&mut self, id: NodeId, dx: f32, dy: f32) {
        let mut stack: Vec<NodeId> = self.nodes[id].children.iter().copied().collect();
        while let Some(c) = stack.pop() {
            if self.nodes[c].element_data().is_none() {
                continue;
            }
            if has_box(&self.nodes[c]) {
                let ld = self.nodes[c].layout_data_mut();
                ld.sticky_offset.x += dx;
                ld.sticky_offset.y += dy;
            } else {
                let more: Vec<NodeId> = self.nodes[c].children.iter().copied().collect();
                stack.extend(more);
            }
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
        Axis { pos, size, margin_start: 0.0, margin_end: 0.0, start_inset: start, end_inset: end, start_wins: true }
    }

    #[test]
    fn sticks_to_top_and_stops_at_containing_block_end() {
        // Box at y=100, 20px tall, containing block 0..=400, viewport scrolled to 150 with top: 10.
        assert_eq!(axis_offset(axis(100.0, 20.0, Some(10.0), None), 150.0, 750.0, 0.0, 400.0), 60.0);
        // Scrolled past the containing block: the box stops at cb_end - size.
        assert_eq!(axis_offset(axis(100.0, 20.0, Some(10.0), None), 600.0, 1200.0, 0.0, 400.0), 280.0);
        // Not yet reached: no shift.
        assert_eq!(axis_offset(axis(100.0, 20.0, Some(10.0), None), 0.0, 600.0, 0.0, 400.0), 0.0);
    }

    #[test]
    fn sticks_to_bottom() {
        // Box at y=900 in a 600px viewport at scroll 0 with bottom: 0 → pulled up to 580.
        assert_eq!(axis_offset(axis(900.0, 20.0, None, Some(0.0)), 0.0, 600.0, 0.0, 1000.0), -320.0);
        // Cannot move above its containing block start.
        assert_eq!(axis_offset(axis(900.0, 20.0, None, Some(0.0)), 0.0, 600.0, 700.0, 1000.0), -200.0);
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
        assert_eq!(overconstraint_winners(WritingMode::HorizontalTb, Direction::Ltr), (true, true));
        assert_eq!(overconstraint_winners(WritingMode::HorizontalTb, Direction::Rtl), (false, true));
        assert_eq!(overconstraint_winners(WritingMode::VerticalRl, Direction::Ltr), (false, true));
        assert_eq!(overconstraint_winners(WritingMode::VerticalLr, Direction::Rtl), (true, false));
    }

    #[test]
    fn taller_than_the_space_between_insets_follows_the_winner() {
        // WPT top-and-bottom-overconstrained: 200px box, scroller 100px, top: 20, bottom: 0,
        // in-flow at y=200 in a 600px containing block.
        let a = |start_wins| Axis { pos: 200.0, size: 200.0, margin_start: 0.0, margin_end: 0.0, start_inset: Some(20.0), end_inset: Some(0.0), start_wins };
        assert_eq!(200.0 + axis_offset(a(true), 0.0, 100.0, 0.0, 600.0), 20.0);
        assert_eq!(200.0 + axis_offset(a(true), 160.0, 260.0, 0.0, 600.0), 180.0);
        assert_eq!(200.0 + axis_offset(a(true), 220.0, 320.0, 0.0, 600.0), 240.0);
        assert_eq!(200.0 + axis_offset(a(true), 500.0, 600.0, 0.0, 600.0), 400.0);
    }

    #[test]
    fn auto_insets_do_nothing() {
        assert_eq!(axis_offset(axis(100.0, 20.0, None, None), 500.0, 1100.0, 0.0, 400.0), 0.0);
    }
}
