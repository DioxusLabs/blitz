pub use stylo_types::*;

mod stylo_types {
    use style::{
        color::AbsoluteColor,
        values::{
            computed::{Angle, AngleOrPercentage, CSSPixelLength, Percentage},
            generics::{
                color::Color,
                image::{GenericGradient, GenericGradientItem},
                position::GenericPosition,
                NonNegative,
            },
        },
        OwnedSlice,
    };

    use style::values::computed::{LengthPercentage, LineDirection};

    /// The type used in the BackgroundImage gradient type
    pub type StyloGradient = GenericGradient<
        LineDirection,
        LengthPercentage,
        NonNegative<CSSPixelLength>,
        NonNegative<LengthPercentage>,
        GenericPosition<LengthPercentage, LengthPercentage>,
        Angle,
        AngleOrPercentage,
        Color<Percentage>,
    >;

    //
    pub type GradientSlice = OwnedSlice<GenericGradientItem<Color<Percentage>, LengthPercentage>>;

    use vello::peniko::Color as VelloColor;

    pub trait ToVelloColor {
        fn as_vello(&self) -> VelloColor;
    }

    impl ToVelloColor for style::values::generics::color::Color<Percentage> {
        fn as_vello(&self) -> VelloColor {
            self.as_absolute()
                .map(|f| f.as_vello())
                .unwrap_or(VelloColor::BLACK)
        }
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
}
