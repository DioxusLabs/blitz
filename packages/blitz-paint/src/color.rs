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

/// The WCAG contrast ratio (>= 1) between two colours, matching Chrome's
/// `color_utils::GetContrastRatio`.
pub(crate) fn contrast_ratio(a: Color, b: Color) -> f32 {
    // `relative_luminance` is defined on `OpaqueColor`; discard the alpha (which
    // it ignores anyway) with `split`.
    let la = a.split().0.relative_luminance() + 0.05;
    let lb = b.split().0.relative_luminance() + 0.05;
    if la > lb { la / lb } else { lb / la }
}

/// Blend `color` towards black or white (whichever has contrast headroom)
/// just far enough to reach `target` contrast with the original, so the
/// result is visibly distinct for any input. Transparent colours pass
/// through unchanged; alpha is preserved.
#[cfg(feature = "scrollbars")]
pub(crate) fn blend_for_contrast(color: Color, target: f32) -> Color {
    let alpha = color.components[3];
    if alpha == 0.0 {
        return color;
    }
    let white = Color::new([1.0, 1.0, 1.0, alpha]);
    let black = Color::new([0.0, 0.0, 0.0, alpha]);
    let pole = if contrast_ratio(color, white) >= contrast_ratio(color, black) {
        white
    } else {
        black
    };
    // Contrast grows monotonically towards the pole: bisect the blend
    // fraction to land just past the target ratio.
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..8 {
        let mid = (lo + hi) / 2.0;
        if contrast_ratio(color, color.lerp_rect(pole, mid)) < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    color.lerp_rect(pole, hi)
}
