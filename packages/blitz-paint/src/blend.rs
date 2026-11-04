pub(crate) use style::computed_values::mix_blend_mode::T as StyloBlendMode;

use peniko::{BlendMode, Compose, Mix};

pub(crate) fn convert_blend_mode(blend: StyloBlendMode) -> BlendMode {
    match blend {
        StyloBlendMode::Normal => Mix::Normal,
        StyloBlendMode::Multiply => Mix::Multiply,
        StyloBlendMode::Screen => Mix::Screen,
        StyloBlendMode::Overlay => Mix::Overlay,
        StyloBlendMode::Darken => Mix::Darken,
        StyloBlendMode::Lighten => Mix::Lighten,
        StyloBlendMode::ColorDodge => Mix::ColorDodge,
        StyloBlendMode::ColorBurn => Mix::ColorBurn,
        StyloBlendMode::HardLight => Mix::HardLight,
        StyloBlendMode::SoftLight => Mix::SoftLight,
        StyloBlendMode::Difference => Mix::Difference,
        StyloBlendMode::Exclusion => Mix::Exclusion,
        StyloBlendMode::Hue => Mix::Hue,
        StyloBlendMode::Saturation => Mix::Saturation,
        StyloBlendMode::Color => Mix::Color,
        StyloBlendMode::Luminosity => Mix::Luminosity,
        StyloBlendMode::PlusLighter => return BlendMode::new(Mix::Normal, Compose::PlusLighter),
    }
    .into()
}
