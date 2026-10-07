//! Transposition of physical styles into the inline/block axes of a writing-mode "frame".
//!
//! Taffy's algorithms treat `width`/x as the inline axis and `height`/+y as the block axis.
//! When a box has a vertical `writing-mode`, Blitz runs Taffy in the box's *frame*: Taffy's x axis
//! is the physical y axis (top to bottom) and Taffy's y axis is the physical block axis of the
//! writing mode (right-to-left for `vertical-rl`/`sideways-rl`, left-to-right for
//! `vertical-lr`/`sideways-lr`). The helpers here map physical style values into that frame.
//!
//! Taffy side -> physical side in a vertical frame:
//!
//! | Taffy        | physical                                         |
//! |--------------|--------------------------------------------------|
//! | `left`       | top                                              |
//! | `right`      | bottom                                           |
//! | `top`        | block-start side (`right` for `*-rl`, `left` for `*-lr`) |
//! | `bottom`     | block-end side                                   |
//!
//! `direction` is `Ltr` when the inline axis runs top-to-bottom (all vertical modes except
//! `sideways-lr`, modulo `direction: rtl`). Line-relative `left`/`right` keywords
//! (`float`, `clear`, `text-align`) refer to the line-left/line-right sides, which are the
//! physical top/bottom for every vertical mode except `sideways-lr`, where they are swapped.

use style::logical_geometry::{PhysicalSide, WritingMode};

/// Whether `frame` swaps Taffy's axes relative to the physical axes.
#[inline(always)]
pub fn is_vertical(frame: WritingMode) -> bool {
    frame.is_vertical()
}

/// Whether the line-left side of a vertical frame is physically the bottom (only `sideways-lr`).
#[inline]
pub fn line_left_is_bottom(frame: WritingMode) -> bool {
    frame.contains(WritingMode::WRITING_MODE_SIDEWAYS_LR)
}

/// The physical side that is Taffy's left (inline-axis start, `x = 0`) in `frame`.
#[inline]
fn taffy_left_side(frame: WritingMode) -> PhysicalSide {
    if frame.is_vertical() {
        PhysicalSide::Top
    } else {
        PhysicalSide::Left
    }
}

/// Taffy `direction` of a node whose own writing mode is `own`, expressed in `frame`: `Ltr` if the
/// node's start edge along the frame's inline axis is Taffy's left side. For a node orthogonal to
/// the frame that edge is its block-start side.
#[inline]
pub fn direction_in(frame: WritingMode, own: WritingMode) -> taffy::Direction {
    let own_start = if own.is_vertical() == frame.is_vertical() {
        own.inline_start_physical_side()
    } else {
        own.block_start_physical_side()
    };
    if own_start == taffy_left_side(frame) {
        taffy::Direction::Ltr
    } else {
        taffy::Direction::Rtl
    }
}

/// Whether the `left` alignment keyword (line-left: the top of a vertical frame, bottom for
/// `sideways-lr`) is Taffy's `end` in `frame`'s inline axis.
#[inline]
pub fn inline_left_is_end(frame: WritingMode) -> bool {
    (direction(frame) == taffy::Direction::Rtl) != line_left_is_bottom(frame)
}

/// Whether the physical `left` is Taffy's `end` in `frame`'s block axis (`vertical-rl`/`sideways-rl`).
#[inline]
pub fn block_left_is_end(frame: WritingMode) -> bool {
    frame.block_start_physical_side() == PhysicalSide::Right
}

/// Whether the start edge of a node with writing mode `own` along `frame`'s block axis is the
/// frame's block-end side: its block-start for parallel writing modes, its inline-start for
/// orthogonal ones.
#[inline]
pub fn block_start_is_reversed(frame: WritingMode, own: WritingMode) -> bool {
    let own_start = if own.is_vertical() == frame.is_vertical() {
        own.block_start_physical_side()
    } else {
        own.inline_start_physical_side()
    };
    own_start != frame.block_start_physical_side()
}

/// Taffy `direction` of a node laid out in its own frame.
#[inline]
pub fn direction(frame: WritingMode) -> taffy::Direction {
    direction_in(frame, frame)
}

/// Map a physical rect (`left`/`right`/`top`/`bottom` are the physical sides) into `frame`.
#[inline]
pub fn rect<T>(frame: WritingMode, physical: taffy::Rect<T>) -> taffy::Rect<T> {
    if !frame.is_vertical() {
        return physical;
    }
    let (block_start, block_end) = if frame.is_vertical_lr() {
        (physical.left, physical.right)
    } else {
        (physical.right, physical.left)
    };
    taffy::Rect {
        left: physical.top,
        right: physical.bottom,
        top: block_start,
        bottom: block_end,
    }
}

/// Map a physical rect in `frame` back to physical sides (the inverse of [`rect`]).
#[inline]
pub fn rect_to_physical<T>(frame: WritingMode, logical: taffy::Rect<T>) -> taffy::Rect<T> {
    if !frame.is_vertical() {
        return logical;
    }
    let (left, right) = if frame.is_vertical_lr() {
        (logical.top, logical.bottom)
    } else {
        (logical.bottom, logical.top)
    };
    taffy::Rect {
        left,
        right,
        top: logical.left,
        bottom: logical.right,
    }
}

/// Map a physical size into `frame` (swap for vertical frames).
#[inline(always)]
pub fn size<T>(frame: WritingMode, physical: taffy::Size<T>) -> taffy::Size<T> {
    if frame.is_vertical() {
        physical.transpose()
    } else {
        physical
    }
}

/// Map a physical point into `frame` (swap for vertical frames).
#[inline(always)]
pub fn point<T>(frame: WritingMode, physical: taffy::Point<T>) -> taffy::Point<T> {
    if frame.is_vertical() {
        physical.transpose()
    } else {
        physical
    }
}

/// Map a physical aspect ratio (width / height) into `frame`.
#[inline(always)]
pub fn aspect_ratio(frame: WritingMode, physical: Option<f32>) -> Option<f32> {
    if frame.is_vertical() {
        physical.map(|ratio| 1.0 / ratio)
    } else {
        physical
    }
}

/// Map a line-relative `float` into `frame`.
#[cfg(feature = "floats")]
#[inline]
pub fn float(frame: WritingMode, float: taffy::Float) -> taffy::Float {
    if line_left_is_bottom(frame) {
        match float {
            taffy::Float::Left => taffy::Float::Right,
            taffy::Float::Right => taffy::Float::Left,
            taffy::Float::None => taffy::Float::None,
        }
    } else {
        float
    }
}

/// Map a line-relative `clear` into `frame`.
#[cfg(feature = "floats")]
#[inline]
pub fn clear(frame: WritingMode, clear: taffy::Clear) -> taffy::Clear {
    if line_left_is_bottom(frame) {
        match clear {
            taffy::Clear::Left => taffy::Clear::Right,
            taffy::Clear::Right => taffy::Clear::Left,
            other => other,
        }
    } else {
        clear
    }
}

/// Map a line-relative legacy `text-align` into `frame`.
#[cfg(feature = "block")]
#[inline]
pub fn text_align(frame: WritingMode, align: taffy::TextAlign) -> taffy::TextAlign {
    if line_left_is_bottom(frame) {
        match align {
            taffy::TextAlign::LegacyLeft => taffy::TextAlign::LegacyRight,
            taffy::TextAlign::LegacyRight => taffy::TextAlign::LegacyLeft,
            other => other,
        }
    } else {
        align
    }
}

/// Transpose a concrete physical [`taffy::Style`] into `frame`.
pub fn transpose_style<S: taffy::CheapCloneStr>(frame: WritingMode, style: &mut taffy::Style<S>) {
    if !frame.is_vertical() {
        return;
    }
    style.direction = direction(frame);
    style.overflow = style.overflow.transpose();
    #[cfg(feature = "floats")]
    {
        style.float = float(frame, style.float);
        style.clear = clear(frame, style.clear);
    }
    #[cfg(feature = "block")]
    {
        style.text_align = text_align(frame, style.text_align);
    }
    style.size = style.size.transpose();
    style.min_size = style.min_size.transpose();
    style.max_size = style.max_size.transpose();
    style.aspect_ratio = aspect_ratio(frame, style.aspect_ratio);
    style.inset = rect(frame, style.inset);
    style.margin = rect(frame, style.margin);
    style.padding = rect(frame, style.padding);
    style.border = rect(frame, style.border);
}
