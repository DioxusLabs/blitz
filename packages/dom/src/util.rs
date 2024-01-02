use app_units::Au;
use style::{
    properties::style_structs::{Margin, Padding},
    values::{
        computed::LengthPercentage,
        generics::{length::GenericLengthPercentageOrAuto, NonNegative},
    },
};
use taffy::prelude::LengthPercentageAuto;

pub fn to_taffy_margin(margin: &Margin) -> taffy::Rect<LengthPercentageAuto> {
    taffy::Rect {
        left: to_taffy_length_percentage_auto(&margin.margin_left),
        right: to_taffy_length_percentage_auto(&margin.margin_right),
        top: to_taffy_length_percentage_auto(&margin.margin_top),
        bottom: to_taffy_length_percentage_auto(&margin.margin_bottom),
    }
}

pub fn to_taffy_padding(padding: &Padding) -> taffy::Rect<taffy::LengthPercentage> {
    taffy::Rect {
        left: ato_taffy_length_percentage_auto(&padding.padding_left),
        right: ato_taffy_length_percentage_auto(&padding.padding_right),
        top: ato_taffy_length_percentage_auto(&padding.padding_top),
        bottom: ato_taffy_length_percentage_auto(&padding.padding_bottom),
    }
}

pub fn to_taffy_border(
    border: &style::properties::style_structs::Border,
) -> taffy::Rect<taffy::LengthPercentage> {
    taffy::Rect {
        left: aato_taffy_length_percentage_auto(&border.border_left_width),
        right: aato_taffy_length_percentage_auto(&border.border_right_width),
        top: aato_taffy_length_percentage_auto(&border.border_top_width),
        bottom: aato_taffy_length_percentage_auto(&border.border_bottom_width),
    }
}

fn aato_taffy_length_percentage_auto(width: &Au) -> taffy::LengthPercentage {
    use taffy::LengthPercentage;
    // returns the nearest pixel
    LengthPercentage::Length(width.to_f32_px())
}

fn to_percent(val: &LengthPercentage) -> LengthPercentageAuto {
    if let Some(length) = val.to_length() {
        LengthPercentageAuto::Length(length.px())
    } else if let Some(val) = val.to_percentage() {
        LengthPercentageAuto::Percent(val.0)
    } else {
        LengthPercentageAuto::Auto
    }
}

fn ato_taffy_length_percentage_auto(
    val: &NonNegative<LengthPercentage>,
) -> taffy::LengthPercentage {
    use taffy::LengthPercentage;

    if let Some(length) = val.0.to_length() {
        LengthPercentage::Length(length.px())
    } else if let Some(val) = val.0.to_percentage() {
        LengthPercentage::Percent(val.0)
    } else {
        LengthPercentage::Percent(0.0)
    }
}

fn to_taffy_length_percentage_auto(
    val: &GenericLengthPercentageOrAuto<LengthPercentage>,
) -> LengthPercentageAuto {
    match val {
        GenericLengthPercentageOrAuto::Auto => LengthPercentageAuto::Auto,
        GenericLengthPercentageOrAuto::LengthPercentage(val) => {
            if let Some(length) = val.to_length() {
                LengthPercentageAuto::Length(length.px())
            } else if let Some(val) = val.to_percentage() {
                LengthPercentageAuto::Percent(val.0)
            } else {
                LengthPercentageAuto::Auto
            }
        }
    }
}
