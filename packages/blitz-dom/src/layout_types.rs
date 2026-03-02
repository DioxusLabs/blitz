//! Layout types that mirror taffy's geometry and layout types.
//!
//! These types decouple blitz-dom's public API from taffy's semver,
//! allowing taffy to be updated without breaking downstream crates.

use std::ops::Add;

/// A 2D point with `x` and `y` coordinates.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Point<T> {
    pub x: T,
    pub y: T,
}

impl Point<f32> {
    /// A zero-valued point.
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
}

impl<T> From<taffy::Point<T>> for Point<T> {
    fn from(p: taffy::Point<T>) -> Self {
        Self { x: p.x, y: p.y }
    }
}

impl<T> From<Point<T>> for taffy::Point<T> {
    fn from(p: Point<T>) -> Self {
        Self { x: p.x, y: p.y }
    }
}

/// A 2D size with `width` and `height`.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Size<T> {
    pub width: T,
    pub height: T,
}

impl Size<f32> {
    /// A zero-valued size.
    pub const ZERO: Self = Self {
        width: 0.0,
        height: 0.0,
    };
}

impl<T> Size<T> {
    /// Apply a mapping function to both width and height.
    pub fn map<R, F: Fn(T) -> R>(self, f: F) -> Size<R> {
        Size {
            width: f(self.width),
            height: f(self.height),
        }
    }
}

impl<T> From<taffy::Size<T>> for Size<T> {
    fn from(s: taffy::Size<T>) -> Self {
        Self {
            width: s.width,
            height: s.height,
        }
    }
}

impl<T> From<Size<T>> for taffy::Size<T> {
    fn from(s: Size<T>) -> Self {
        Self {
            width: s.width,
            height: s.height,
        }
    }
}

/// A rectangle defined by its edges: `left`, `right`, `top`, `bottom`.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Rect<T> {
    pub left: T,
    pub right: T,
    pub top: T,
    pub bottom: T,
}

impl Rect<f32> {
    /// A zero-valued rect.
    pub const ZERO: Self = Self {
        left: 0.0,
        right: 0.0,
        top: 0.0,
        bottom: 0.0,
    };
}

impl<T> Rect<T> {
    /// Apply a mapping function to all four edges.
    pub fn map<R, F: Fn(T) -> R>(self, f: F) -> Rect<R> {
        Rect {
            left: f(self.left),
            right: f(self.right),
            top: f(self.top),
            bottom: f(self.bottom),
        }
    }
}

impl<U, T: Add<U>> Add<Rect<U>> for Rect<T> {
    type Output = Rect<T::Output>;

    fn add(self, rhs: Rect<U>) -> Self::Output {
        Rect {
            left: self.left + rhs.left,
            right: self.right + rhs.right,
            top: self.top + rhs.top,
            bottom: self.bottom + rhs.bottom,
        }
    }
}

impl<T> From<taffy::Rect<T>> for Rect<T> {
    fn from(r: taffy::Rect<T>) -> Self {
        Self {
            left: r.left,
            right: r.right,
            top: r.top,
            bottom: r.bottom,
        }
    }
}

impl<T> From<Rect<T>> for taffy::Rect<T> {
    fn from(r: Rect<T>) -> Self {
        Self {
            left: r.left,
            right: r.right,
            top: r.top,
            bottom: r.bottom,
        }
    }
}

/// The final result of a layout algorithm for a single node.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Layout {
    /// The relative ordering of the node.
    pub order: u32,
    /// The top-left corner of the node relative to its parent.
    pub location: Point<f32>,
    /// The width and height of the node.
    pub size: Size<f32>,
    /// The size of the content inside the node (may be larger than `size` for overflow).
    pub content_size: Size<f32>,
    /// The size of the scrollbars in each dimension.
    pub scrollbar_size: Size<f32>,
    /// The size of the borders of the node.
    pub border: Rect<f32>,
    /// The size of the padding of the node.
    pub padding: Rect<f32>,
    /// The size of the margin of the node.
    pub margin: Rect<f32>,
}

impl Default for Layout {
    fn default() -> Self {
        Self::new()
    }
}

impl Layout {
    /// Creates a new zero-layout.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            order: 0,
            location: Point::ZERO,
            size: Size::ZERO,
            content_size: Size::ZERO,
            scrollbar_size: Size::ZERO,
            border: Rect::ZERO,
            padding: Rect::ZERO,
            margin: Rect::ZERO,
        }
    }

    /// Creates a new zero-layout with the supplied `order` value.
    #[must_use]
    pub const fn with_order(order: u32) -> Self {
        Self {
            order,
            location: Point::ZERO,
            size: Size::ZERO,
            content_size: Size::ZERO,
            scrollbar_size: Size::ZERO,
            border: Rect::ZERO,
            padding: Rect::ZERO,
            margin: Rect::ZERO,
        }
    }

    /// Get the width of the node's content box.
    #[inline]
    pub fn content_box_width(&self) -> f32 {
        self.size.width
            - self.padding.left
            - self.padding.right
            - self.border.left
            - self.border.right
    }

    /// Get the height of the node's content box.
    #[inline]
    pub fn content_box_height(&self) -> f32 {
        self.size.height
            - self.padding.top
            - self.padding.bottom
            - self.border.top
            - self.border.bottom
    }

    /// Get the size of the node's content box.
    #[inline]
    pub fn content_box_size(&self) -> Size<f32> {
        Size {
            width: self.content_box_width(),
            height: self.content_box_height(),
        }
    }

    /// Get x offset of the node's content box relative to its parent's border box.
    pub fn content_box_x(&self) -> f32 {
        self.location.x + self.border.left + self.padding.left
    }

    /// Get y offset of the node's content box relative to its parent's border box.
    pub fn content_box_y(&self) -> f32 {
        self.location.y + self.border.top + self.padding.top
    }

    /// Return the scroll width of the node.
    pub fn scroll_width(&self) -> f32 {
        f32::max(
            0.0,
            self.content_size.width + f32::min(self.scrollbar_size.width, self.size.width)
                - self.size.width
                + self.border.right,
        )
    }

    /// Return the scroll height of the node.
    pub fn scroll_height(&self) -> f32 {
        f32::max(
            0.0,
            self.content_size.height + f32::min(self.scrollbar_size.height, self.size.height)
                - self.size.height
                + self.border.bottom,
        )
    }
}

impl From<taffy::Layout> for Layout {
    fn from(l: taffy::Layout) -> Self {
        Self {
            order: l.order,
            location: l.location.into(),
            size: l.size.into(),
            content_size: l.content_size.into(),
            scrollbar_size: l.scrollbar_size.into(),
            border: l.border.into(),
            padding: l.padding.into(),
            margin: l.margin.into(),
        }
    }
}

impl From<Layout> for taffy::Layout {
    fn from(l: Layout) -> Self {
        Self {
            order: l.order,
            location: l.location.into(),
            size: l.size.into(),
            content_size: l.content_size.into(),
            scrollbar_size: l.scrollbar_size.into(),
            border: l.border.into(),
            padding: l.padding.into(),
            margin: l.margin.into(),
        }
    }
}
