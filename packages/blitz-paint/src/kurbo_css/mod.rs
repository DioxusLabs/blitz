//! A rounded rect closer to the browser
//! Implemented in such a way that splits the border into 4 parts at the midway of each radius
//!
//! This object is meant to be updated only when the data changes - BezPaths are expensive!
//!
//! Can I just say, this is a lot of work for a border
//! HTML/css is annoyingly wild

use kurbo::{Insets, Vec2};

mod css_rect;
mod non_uniform_radii;

pub use css_rect::CssRect;
pub use non_uniform_radii::NonUniformRoundedRectRadii;

#[derive(Debug, Clone, Copy)]
pub enum Edge {
    Top,
    Right,
    Bottom,
    Left,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(clippy::enum_variant_names, reason = "Use CSS standard terminology")]
pub(crate) enum CssBox {
    OutlineBox,
    BorderBox,
    PaddingBox,
    ContentBox,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Direction {
    Clockwise,
    Anticlockwise,
}

fn add_insets(a: Insets, b: Insets) -> Insets {
    Insets {
        x0: a.x0 + b.x0,
        y0: a.y0 + b.y0,
        x1: a.x1 + b.x1,
        y1: a.y1 + b.y1,
    }
}

#[inline(always)]
fn get_corner_insets(insets: Insets, corner: Corner) -> Vec2 {
    match corner {
        Corner::TopLeft => Vec2 {
            x: insets.x0,
            y: insets.y0,
        },
        Corner::TopRight => Vec2 {
            x: insets.x1,
            y: insets.y0,
        },
        Corner::BottomLeft => Vec2 {
            x: insets.x0,
            y: insets.y1,
        },
        Corner::BottomRight => Vec2 {
            x: insets.x1,
            y: insets.y1,
        },
    }
}
