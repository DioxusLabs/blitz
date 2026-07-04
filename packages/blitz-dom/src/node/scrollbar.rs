//! Overlay scrollbar geometry and css-scrollbars-1 style accessors for
//! [`Node`]. Geometry is shared between painting (blitz-paint) and thumb
//! hit-testing so the two cannot drift.

use kurbo::Rect as KurboRect;
use taffy::AbsoluteAxis;

use super::Node;

/// A specific scrollbar: one axis of one scroll container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollbarRef {
    pub node_id: usize,
    pub axis: AbsoluteAxis,
}

/// The computed value of `scrollbar-width` (css-scrollbars-1). A local
/// mirror of the stylo type, which isn't exposed to the servo engine yet
/// (servo/stylo#413).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollbarWidth {
    #[default]
    Auto,
    Thin,
    None,
}

/// The computed value of `scrollbar-color` (css-scrollbars-1). A local
/// mirror of the stylo type, which isn't exposed to the servo engine yet
/// (servo/stylo#413). Colors are fully resolved (no `currentColor`).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum ScrollbarColor {
    #[default]
    Auto,
    Colors {
        thumb: style::color::AbsoluteColor,
        track: style::color::AbsoluteColor,
    },
}

impl Node {
    /// The node's used `scrollbar-width`.
    pub fn scrollbar_width(&self) -> ScrollbarWidth {
        // TODO: read the computed style once stylo exposes scrollbar-width
        // to the servo engine (servo/stylo#413):
        // match self.primary_styles().map(|s| s.clone_scrollbar_width()) { .. }
        ScrollbarWidth::Auto
    }

    /// The node's used `scrollbar-color`.
    pub fn scrollbar_color(&self) -> ScrollbarColor {
        // TODO: read the computed style once stylo exposes scrollbar-color
        // to the servo engine (servo/stylo#413), resolving the colors
        // against the element's `color`:
        // self.primary_styles().map(|s| s.clone_scrollbar_color()) { .. }
        ScrollbarColor::Auto
    }

    /// Whether the node shows an overlay scrollbar in the given axis:
    /// always for `overflow: scroll`, only when the content overflows for
    /// `overflow: auto`, never otherwise — and never when
    /// `scrollbar-width: none`.
    pub fn wants_scrollbar(&self, axis: AbsoluteAxis) -> bool {
        use style::values::computed::Overflow;
        let Some(style) = self.primary_styles() else {
            return false;
        };
        if self.scrollbar_width() == ScrollbarWidth::None {
            return false;
        }
        let (overflow, scroll_extent) = match axis {
            AbsoluteAxis::Horizontal => (
                style.clone_overflow_x(),
                self.final_layout.scroll_width() as f64,
            ),
            AbsoluteAxis::Vertical => (
                style.clone_overflow_y(),
                self.final_layout.scroll_height() as f64,
            ),
        };
        match overflow {
            Overflow::Scroll => true,
            Overflow::Auto => scroll_extent > 0.5,
            _ => false,
        }
    }

    /// The scrollport (padding box) in (unscaled) CSS px relative to the
    /// node's border-box origin. Taffy has content-box helpers but none for
    /// the padding box.
    fn scrollport(&self) -> KurboRect {
        let layout = &self.final_layout;
        KurboRect::new(
            layout.border.left as f64,
            layout.border.top as f64,
            layout.size.width as f64 - layout.border.right as f64,
            layout.size.height as f64 - layout.border.bottom as f64,
        )
    }

    /// Geometry of the overlay scrollbar thumb for the given axis, in
    /// (unscaled) CSS px relative to the node's border-box origin. `None`
    /// if there is no scrollable overflow in that axis.
    pub fn scrollbar_thumb(&self, axis: AbsoluteAxis) -> Option<KurboRect> {
        // Matches Chromium's overlay thumb in its interactive state.
        const THUMB_THICKNESS: f64 = 10.0;
        const THIN_THUMB_THICKNESS: f64 = 6.0;
        const THUMB_MARGIN: f64 = 2.0;
        const MIN_THUMB_LENGTH: f64 = 32.0;

        let layout = &self.final_layout;
        let scroll_extent = match axis {
            AbsoluteAxis::Horizontal => layout.scroll_width() as f64,
            AbsoluteAxis::Vertical => layout.scroll_height() as f64,
        };
        if scroll_extent <= 0.5 {
            return None;
        }

        let thickness = match self.scrollbar_width() {
            ScrollbarWidth::Thin => THIN_THUMB_THICKNESS,
            _ => THUMB_THICKNESS,
        };

        let port = self.scrollport();
        let (viewport_len, scroll_offset) = match axis {
            AbsoluteAxis::Horizontal => (port.width(), self.scroll_offset.x),
            AbsoluteAxis::Vertical => (port.height(), self.scroll_offset.y),
        };
        let thumb_len = (viewport_len * viewport_len / (viewport_len + scroll_extent))
            .max(MIN_THUMB_LENGTH)
            .min(viewport_len);
        let progress = (scroll_offset / scroll_extent).clamp(0.0, 1.0);
        // Round a sub-pixel displacement up to a whole pixel so any nonzero
        // scroll visibly moves the thumb off the origin.
        let thumb_start = match progress * (viewport_len - thumb_len) {
            start if start > 0.0 && start < 1.0 => 1.0,
            start => start,
        };

        Some(match axis {
            AbsoluteAxis::Horizontal => KurboRect::new(
                port.x0 + thumb_start,
                port.y1 - THUMB_MARGIN - thickness,
                port.x0 + thumb_start + thumb_len,
                port.y1 - THUMB_MARGIN,
            ),
            AbsoluteAxis::Vertical => KurboRect::new(
                port.x1 - THUMB_MARGIN - thickness,
                port.y0 + thumb_start,
                port.x1 - THUMB_MARGIN,
                port.y0 + thumb_start + thumb_len,
            ),
        })
    }

    /// Content px scrolled per thumb px dragged, for the given axis.
    pub fn scrollbar_drag_ratio(&self, axis: AbsoluteAxis) -> f64 {
        let Some(thumb) = self.scrollbar_thumb(axis) else {
            return 0.0;
        };
        let port = self.scrollport();
        let (scroll_extent, viewport_len, thumb_len) = match axis {
            AbsoluteAxis::Horizontal => (
                self.final_layout.scroll_width() as f64,
                port.width(),
                thumb.width(),
            ),
            AbsoluteAxis::Vertical => (
                self.final_layout.scroll_height() as f64,
                port.height(),
                thumb.height(),
            ),
        };
        let track_play = viewport_len - thumb_len;
        if track_play <= 0.0 {
            return 0.0;
        }
        scroll_extent / track_play
    }

    /// The scrollbar thumb containing the given point (in this node's
    /// border-box coordinates), if any. The `scrollbars` feature's single
    /// behavioral gate: returning `None` keeps unpainted thumbs from ever
    /// claiming pointer events.
    pub(crate) fn scrollbar_at_local(&self, x: f64, y: f64) -> Option<ScrollbarRef> {
        if !cfg!(feature = "scrollbars") {
            return None;
        }
        for axis in [AbsoluteAxis::Vertical, AbsoluteAxis::Horizontal] {
            if !self.wants_scrollbar(axis) {
                continue;
            }
            if let Some(thumb) = self.scrollbar_thumb(axis)
                && thumb.contains(kurbo::Point::new(x, y))
            {
                return Some(ScrollbarRef {
                    node_id: self.id,
                    axis,
                });
            }
        }
        None
    }
}
