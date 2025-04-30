use color::{AlphaColor, DynamicColor, Srgb};
use style::color::AbsoluteColor;

pub type Color = AlphaColor<Srgb>;

pub trait ToColorColor {
    /// Converts a color into the `AlphaColor<Srgb>` type from the `color` crate
    fn as_srgb_color(&self) -> Color;

    /// Converts a color into the `DynamicColor` type from the `color` crate
    fn as_dynamic_color(&self) -> DynamicColor;
}
impl ToColorColor for AbsoluteColor {
    fn as_srgb_color(&self) -> Color {
        Color::new(
            *self
                .to_color_space(style::color::ColorSpace::Srgb)
                .raw_components(),
        )
    }

    fn as_dynamic_color(&self) -> DynamicColor {
        DynamicColor::from_alpha_color(self.as_srgb_color())
    }
}
