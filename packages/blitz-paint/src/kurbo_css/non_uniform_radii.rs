use kurbo::Vec2;
use std::ops::{Mul, MulAssign};

/// Radii for each corner of a non-uniform rounded rectangle.
///
/// The use of `top` as in `top_left` assumes a y-down coordinate space. Piet
/// (and Druid by extension) uses a y-down coordinate space, but Kurbo also
/// supports a y-up coordinate space, in which case `top_left` would actually
/// refer to the bottom-left corner, and vice versa. Top may not always
/// actually be the top, but `top` corners will always have a smaller y-value
/// than `bottom` corners.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
// #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
// #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NonUniformRoundedRectRadii {
    /// The radii of the top-left corner.
    pub top_left: Vec2,
    /// The radii of the top-right corner.
    pub top_right: Vec2,
    /// The radii of the bottom-right corner.
    pub bottom_right: Vec2,
    /// The radii of the bottom-left corner.
    pub bottom_left: Vec2,
}

impl NonUniformRoundedRectRadii {
    pub fn average(&self) -> f64 {
        (self.top_left.x
            + self.top_left.y
            + self.top_right.x
            + self.top_right.y
            + self.bottom_left.x
            + self.bottom_left.y
            + self.bottom_right.x
            + self.bottom_right.y)
            / 8.0
    }
}

impl Mul<f64> for NonUniformRoundedRectRadii {
    type Output = Self;

    fn mul(self, rhs: f64) -> Self::Output {
        Self {
            top_left: self.top_left * rhs,
            top_right: self.top_right * rhs,
            bottom_right: self.bottom_right * rhs,
            bottom_left: self.bottom_left * rhs,
        }
    }
}

impl MulAssign<f64> for NonUniformRoundedRectRadii {
    fn mul_assign(&mut self, rhs: f64) {
        self.top_left *= rhs;
        self.top_right *= rhs;
        self.bottom_left *= rhs;
        self.bottom_right *= rhs;
    }
}
