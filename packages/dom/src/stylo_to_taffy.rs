//! Conversion functions from Stylo types to Taffy types

// Module of type aliases so we can refer to stylo types with nicer names
mod stylo {
    pub(crate) use style::computed_values::align_content::T as AlignContent;
    pub(crate) use style::computed_values::align_items::T as AlignItems;
    pub(crate) use style::computed_values::align_self::T as AlignSelf;
    pub(crate) use style::computed_values::flex_direction::T as FlexDirection;
    pub(crate) use style::computed_values::flex_wrap::T as FlexWrap;
    pub(crate) use style::computed_values::justify_content::T as JustifyContent;
    // pub(crate) use style::computed_values::justify_items::T as JustifyItems;
    // pub(crate) use style::computed_values::justify_self::T as JustifySelf;
    pub(crate) use style::properties::longhands::aspect_ratio::computed_value::T as AspectRatio;
    pub(crate) use style::properties::longhands::position::computed_value::T as Position;
    pub(crate) use style::properties::style_structs::{Margin, Padding};
    pub(crate) use style::values::computed::LengthPercentage;
    pub(crate) use style::values::generics::flex::GenericFlexBasis;
    pub(crate) use style::values::generics::length::GenericLengthPercentageOrAuto;
    pub(crate) use style::values::generics::length::GenericLengthPercentageOrNormal;
    pub(crate) use style::values::generics::length::GenericMaxSize;
    pub(crate) use style::values::generics::length::GenericSize;
    pub(crate) use style::values::generics::position::PreferredRatio;
    pub(crate) use style::values::generics::NonNegative;
    pub(crate) use style::values::specified::box_::Display;
    pub(crate) use style::values::specified::box_::DisplayInside;
    pub(crate) use style::values::specified::box_::DisplayOutside;
    pub(crate) use style::values::specified::box_::Overflow;
    pub(crate) type LengthPercentageAuto = GenericLengthPercentageOrAuto<LengthPercentage>;
    pub(crate) type Size = GenericSize<NonNegative<LengthPercentage>>;
    pub(crate) type MaxSize = GenericMaxSize<NonNegative<LengthPercentage>>;
    pub(crate) type FlexBasis = GenericFlexBasis<Size>;
    pub(crate) type Gap = GenericLengthPercentageOrNormal<NonNegative<LengthPercentage>>;
}

pub(crate) fn length_percentage(val: &stylo::LengthPercentage) -> taffy::LengthPercentage {
    if let Some(length) = val.to_length() {
        taffy::LengthPercentage::Length(length.px())
    } else if let Some(val) = val.to_percentage() {
        taffy::LengthPercentage::Percent(val.0)
    } else {
        // TODO: Support calc
        taffy::LengthPercentage::Percent(0.0)
    }
}

pub(crate) fn length_percentage_auto(
    val: &stylo::LengthPercentageAuto,
) -> taffy::LengthPercentageAuto {
    match val {
        stylo::LengthPercentageAuto::Auto => taffy::LengthPercentageAuto::Auto,
        stylo::LengthPercentageAuto::LengthPercentage(val) => length_percentage(val).into(),
    }
}

pub(crate) fn dimension(val: &stylo::Size) -> taffy::Dimension {
    match val {
        stylo::Size::LengthPercentage(val) => length_percentage(&val.0).into(),
        stylo::Size::Auto => taffy::Dimension::Auto,
        // TODO: implement other values in Taffy (and servo configuration of stylo)
        // _ => taffy::Dimension::Auto,
    }
}

pub(crate) fn max_size_dimension(val: &stylo::MaxSize) -> taffy::Dimension {
    match val {
        stylo::MaxSize::LengthPercentage(val) => length_percentage(&val.0).into(),
        stylo::MaxSize::None => taffy::Dimension::Auto,
        // TODO: implement other values in Taffy (and servo configuration of stylo)
        // _ => taffy::Dimension::Auto,
    }
}

pub(crate) fn margin(margin: &stylo::Margin) -> taffy::Rect<taffy::LengthPercentageAuto> {
    taffy::Rect {
        left: length_percentage_auto(&margin.margin_left),
        right: length_percentage_auto(&margin.margin_right),
        top: length_percentage_auto(&margin.margin_top),
        bottom: length_percentage_auto(&margin.margin_bottom),
    }
}

pub(crate) fn padding(padding: &stylo::Padding) -> taffy::Rect<taffy::LengthPercentage> {
    taffy::Rect {
        left: length_percentage(&padding.padding_left.0),
        right: length_percentage(&padding.padding_right.0),
        top: length_percentage(&padding.padding_top.0),
        bottom: length_percentage(&padding.padding_bottom.0),
    }
}

pub(crate) fn border(
    border: &style::properties::style_structs::Border,
) -> taffy::Rect<taffy::LengthPercentage> {
    taffy::Rect {
        left: taffy::LengthPercentage::Length(border.border_left_width.to_f32_px()),
        right: taffy::LengthPercentage::Length(border.border_right_width.to_f32_px()),
        top: taffy::LengthPercentage::Length(border.border_top_width.to_f32_px()),
        bottom: taffy::LengthPercentage::Length(border.border_bottom_width.to_f32_px()),
    }
}

pub(crate) fn display(input: stylo::Display) -> taffy::Display {
    let mut display = match input.inside() {
        stylo::DisplayInside::None => taffy::Display::None,
        stylo::DisplayInside::Flex => taffy::Display::Flex,
        stylo::DisplayInside::Flow => taffy::Display::Block,
        stylo::DisplayInside::FlowRoot => taffy::Display::Block,
        // TODO: Support grid layout in servo configuration of stylo
        // TODO: Support display:contents in Taffy
        // TODO: Support table layout in Taffy
        _ => {
            println!("FALLBACK {:?} {:?}", input.inside(), input.outside());
            taffy::Display::Block
        }
    };

    match input.outside() {
        // This is probably redundant as I suspect display.inside() is always None
        // when display.outside() is None.
        stylo::DisplayOutside::None => display = taffy::Display::None,

        // TODO: Support flow and table layout
        stylo::DisplayOutside::Inline => {}
        stylo::DisplayOutside::Block => {}
        stylo::DisplayOutside::TableCaption => {}
        stylo::DisplayOutside::InternalTable => {}
    };

    display
}

pub(crate) fn position(input: stylo::Position) -> taffy::Position {
    match input {
        // TODO: support position:static
        stylo::Position::Relative => taffy::Position::Relative,
        stylo::Position::Static => taffy::Position::Relative,

        // TODO: support position:fixed and sticky
        stylo::Position::Absolute => taffy::Position::Absolute,
        stylo::Position::Fixed => taffy::Position::Absolute,
        stylo::Position::Sticky => taffy::Position::Absolute,
    }
}

pub(crate) fn overflow(input: stylo::Overflow) -> taffy::Overflow {
    // TODO: Enable Overflow::Clip in servo configuration of stylo
    match input {
        stylo::Overflow::Visible => taffy::Overflow::Visible,
        stylo::Overflow::Hidden => taffy::Overflow::Hidden,
        stylo::Overflow::Scroll => taffy::Overflow::Scroll,
        // TODO: Support Overflow::Auto in Taffy
        stylo::Overflow::Auto => taffy::Overflow::Scroll,
    }
}

pub(crate) fn aspect_ratio(input: stylo::AspectRatio) -> Option<f32> {
    match input.ratio {
        stylo::PreferredRatio::None => None,
        stylo::PreferredRatio::Ratio(val) => Some(val.0.into()),
    }
}

pub(crate) fn gap(input: &stylo::Gap) -> taffy::LengthPercentage {
    match input {
        // For Flexbox and CSS Grid the "normal" value is 0px. This may need to be updated
        // if we ever implement multi-column layout.
        stylo::Gap::Normal => taffy::LengthPercentage::Length(0.0),
        stylo::Gap::LengthPercentage(val) => length_percentage(&val.0),
    }
}

pub(crate) fn flex_basis(input: &stylo::FlexBasis) -> taffy::Dimension {
    // TODO: Support flex-basis: content in Taffy
    match input {
        stylo::FlexBasis::Content => taffy::Dimension::Auto,
        stylo::FlexBasis::Size(size) => dimension(size),
    }
}

pub(crate) fn flex_direction(input: stylo::FlexDirection) -> taffy::FlexDirection {
    match input {
        stylo::FlexDirection::Row => taffy::FlexDirection::Row,
        stylo::FlexDirection::RowReverse => taffy::FlexDirection::RowReverse,
        stylo::FlexDirection::Column => taffy::FlexDirection::Column,
        stylo::FlexDirection::ColumnReverse => taffy::FlexDirection::ColumnReverse,
    }
}

pub(crate) fn flex_wrap(input: stylo::FlexWrap) -> taffy::FlexWrap {
    match input {
        stylo::FlexWrap::Wrap => taffy::FlexWrap::Wrap,
        stylo::FlexWrap::WrapReverse => taffy::FlexWrap::WrapReverse,
        stylo::FlexWrap::Nowrap => taffy::FlexWrap::NoWrap,
    }
}

pub(crate) fn justify_content(input: stylo::JustifyContent) -> Option<taffy::JustifyContent> {
    match input {
        stylo::JustifyContent::Start => Some(taffy::JustifyContent::Start),
        stylo::JustifyContent::End => Some(taffy::JustifyContent::End),
        stylo::JustifyContent::FlexStart => Some(taffy::JustifyContent::FlexStart),
        stylo::JustifyContent::Stretch => Some(taffy::JustifyContent::Stretch),
        stylo::JustifyContent::FlexEnd => Some(taffy::JustifyContent::FlexEnd),
        stylo::JustifyContent::Center => Some(taffy::JustifyContent::Center),
        stylo::JustifyContent::SpaceBetween => Some(taffy::JustifyContent::SpaceBetween),
        stylo::JustifyContent::SpaceAround => Some(taffy::JustifyContent::SpaceAround),
        stylo::JustifyContent::SpaceEvenly => Some(taffy::JustifyContent::SpaceEvenly),
    }
}

pub(crate) fn align_content(input: stylo::AlignContent) -> Option<taffy::AlignContent> {
    match input {
        stylo::AlignContent::Start => Some(taffy::AlignContent::Start),
        stylo::AlignContent::End => Some(taffy::AlignContent::End),
        stylo::AlignContent::FlexStart => Some(taffy::AlignContent::FlexStart),
        stylo::AlignContent::Stretch => Some(taffy::AlignContent::Stretch),
        stylo::AlignContent::FlexEnd => Some(taffy::AlignContent::FlexEnd),
        stylo::AlignContent::Center => Some(taffy::AlignContent::Center),
        stylo::AlignContent::SpaceBetween => Some(taffy::AlignContent::SpaceBetween),
        stylo::AlignContent::SpaceAround => Some(taffy::AlignContent::SpaceAround),
        stylo::AlignContent::SpaceEvenly => Some(taffy::AlignContent::SpaceEvenly),
    }
}

pub(crate) fn align_items(input: stylo::AlignItems) -> Option<taffy::AlignItems> {
    match input {
        stylo::AlignItems::Stretch => Some(taffy::AlignItems::Stretch),
        stylo::AlignItems::FlexStart => Some(taffy::AlignItems::FlexStart),
        stylo::AlignItems::FlexEnd => Some(taffy::AlignItems::FlexEnd),
        stylo::AlignItems::Center => Some(taffy::AlignItems::Center),
        stylo::AlignItems::Baseline => Some(taffy::AlignItems::Baseline),
    }
}

pub(crate) fn align_self(input: stylo::AlignSelf) -> Option<taffy::AlignSelf> {
    match input {
        stylo::AlignSelf::Auto => None,
        stylo::AlignSelf::Stretch => Some(taffy::AlignSelf::Stretch),
        stylo::AlignSelf::FlexStart => Some(taffy::AlignSelf::FlexStart),
        stylo::AlignSelf::FlexEnd => Some(taffy::AlignSelf::FlexEnd),
        stylo::AlignSelf::Center => Some(taffy::AlignSelf::Center),
        stylo::AlignSelf::Baseline => Some(taffy::AlignSelf::Baseline),
    }
}

// pub(crate) fn justify_items(input: stylo::JustifyItems) -> Option<taffy::JustifyItems> {
//     match input {
//         stylo::JustifyItems::Stretch => Some(taffy::JustifyItems::Stretch),
//         stylo::JustifyItems::FlexStart => Some(taffy::JustifyItems::FlexStart),
//         stylo::JustifyItems::FlexEnd => Some(taffy::JustifyItems::FlexEnd),
//         stylo::JustifyItems::Center => Some(taffy::JustifyItems::Center),
//         stylo::JustifyItems::Baseline => Some(taffy::JustifyItems::Baseline),
//     }
// }

// pub(crate) fn justify_self(input: stylo::JustifySelf) -> Option<taffy::JustifySelf> {
//     match input {
//         stylo::JustifySelf::Auto => None,
//         stylo::JustifySelf::Stretch => Some(taffy::JustifySelf::Stretch),
//         stylo::JustifySelf::FlexStart => Some(taffy::JustifySelf::FlexStart),
//         stylo::JustifySelf::FlexEnd => Some(taffy::JustifySelf::FlexEnd),
//         stylo::JustifySelf::Center => Some(taffy::JustifySelf::Center),
//         stylo::JustifySelf::Baseline => Some(taffy::JustifySelf::Baseline),
//     }
// }
