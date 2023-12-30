pub use stylo_types::*;

mod stylo_types {
    use style::{
        color::AbsoluteColor,
        values::{
            computed::{Angle, AngleOrPercentage, CSSPixelLength, Percentage},
            generics::{
                color::Color, image::GenericGradient, position::GenericPosition, NonNegative,
            },
        },
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
            VelloColor {
                r: (self.components.0 * 255.0) as u8,
                g: (self.components.1 * 255.0) as u8,
                b: (self.components.2 * 255.0) as u8,
                a: (self.alpha() * 255.0) as u8,
            }
        }
    }
}
