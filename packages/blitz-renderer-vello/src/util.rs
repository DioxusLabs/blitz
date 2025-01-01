use style::color::AbsoluteColor;
use vello::peniko::Color as VelloColor;

pub trait ToVelloColor {
    fn as_vello(&self) -> VelloColor;
}
impl ToVelloColor for AbsoluteColor {
    fn as_vello(&self) -> VelloColor {
        let [r, g, b, a] = self
            .to_color_space(style::color::ColorSpace::Srgb)
            .raw_components()
            .map(|f| (f * 255.0) as u8);
        VelloColor { r, g, b, a }
    }
}
