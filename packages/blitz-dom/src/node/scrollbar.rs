//! Overlay scrollbar geometry and css-scrollbars-1 style accessors for
//! [`Node`]. Geometry is shared between painting (blitz-paint) and thumb
//! hit-testing so the two cannot drift.

use blitz_traits::node_id::NodeId;
use kurbo::Rect as KurboRect;
use taffy::AbsoluteAxis;
use web_time::Duration;

use super::Node;

/// How long overlay scrollbars stay fully opaque after their last activity
/// (a scroll, or the pointer leaving the thumb), and how long the fade-out
/// takes. Chromium's overlay values.
pub(crate) const FADE_DELAY: Duration = Duration::from_millis(500);
pub(crate) const FADE_DURATION: Duration = Duration::from_millis(200);

/// Overlay scrollbar opacity as a function of time since the scroll
/// container's last scrollbar activity: fully opaque through the fade
/// delay, then fading linearly to hidden.
pub(crate) fn opacity_at(elapsed: Duration) -> f32 {
    match elapsed.checked_sub(FADE_DELAY) {
        None => 1.0,
        Some(fading) => 1.0 - (fading.as_secs_f32() / FADE_DURATION.as_secs_f32()).min(1.0),
    }
}

/// A specific scrollbar: one axis of one scroll container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollbarRef {
    pub node_id: NodeId,
    pub axis: AbsoluteAxis,
}

/// The computed value of `scrollbar-width` (css-scrollbars-1).
pub use style::properties::generated::longhands::scrollbar_width::computed_value::T as ScrollbarWidth;

/// The computed value of `scrollbar-color` (css-scrollbars-1), with colors
/// fully resolved (no `currentColor`).
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
    /// The node's computed `scrollbar-width`.
    pub fn scrollbar_width(&self) -> ScrollbarWidth {
        self.primary_styles()
            .map(|style| style.clone_scrollbar_width())
            .unwrap_or(ScrollbarWidth::Auto)
    }

    /// The node's computed `scrollbar-color`, with colors resolved against
    /// the element's computed `color`.
    pub fn scrollbar_color(&self) -> ScrollbarColor {
        let Some(style) = self.primary_styles() else {
            return ScrollbarColor::Auto;
        };
        let current_color = style.clone_color();
        match style.clone_scrollbar_color() {
            style::values::computed::ScrollbarColor::Auto => ScrollbarColor::Auto,
            style::values::computed::ScrollbarColor::Colors { thumb, track } => {
                ScrollbarColor::Colors {
                    thumb: thumb.resolve_to_absolute(&current_color),
                    track: track.resolve_to_absolute(&current_color),
                }
            }
        }
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
                self.final_layout().scroll_width() as f64,
            ),
            AbsoluteAxis::Vertical => (
                style.clone_overflow_y(),
                self.final_layout().scroll_height() as f64,
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
        let layout = self.final_layout();
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

        let layout = self.final_layout();
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
            AbsoluteAxis::Horizontal => (port.width(), self.scroll_offset().x),
            AbsoluteAxis::Vertical => (port.height(), self.scroll_offset().y),
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
                self.final_layout().scroll_width() as f64,
                port.width(),
                thumb.width(),
            ),
            AbsoluteAxis::Vertical => (
                self.final_layout().scroll_height() as f64,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opacity_holds_through_the_fade_delay_then_fades_out() {
        assert_eq!(opacity_at(Duration::ZERO), 1.0);
        assert_eq!(opacity_at(FADE_DELAY), 1.0);
        let mid_fade = opacity_at(FADE_DELAY + FADE_DURATION / 2);
        assert!((mid_fade - 0.5).abs() < 0.01, "got {mid_fade}");
        assert_eq!(opacity_at(FADE_DELAY + FADE_DURATION), 0.0);
        assert_eq!(opacity_at(Duration::from_secs(3600)), 0.0);
    }
}
