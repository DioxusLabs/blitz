//! Conversion functions from Stylo types to Taffy types

// Module of type aliases so we can refer to stylo types with nicer names
mod stylo {
    pub(crate) use style::computed_values::flex_direction::T as FlexDirection;
    pub(crate) use style::computed_values::flex_wrap::T as FlexWrap;
    pub(crate) use style::computed_values::grid_auto_flow::T as GridAutoFlow;
    pub(crate) use style::properties::longhands::aspect_ratio::computed_value::T as AspectRatio;
    pub(crate) use style::properties::longhands::position::computed_value::T as Position;
    pub(crate) use style::properties::style_structs::{Margin, Padding};
    pub(crate) use style::values::computed::GridLine;
    pub(crate) use style::values::computed::GridTemplateComponent;
    pub(crate) use style::values::computed::ImplicitGridTracks;
    pub(crate) use style::values::computed::LengthPercentage;
    pub(crate) use style::values::generics::flex::GenericFlexBasis;
    pub(crate) use style::values::generics::grid::RepeatCount;
    pub(crate) use style::values::generics::grid::TrackBreadth;
    pub(crate) use style::values::generics::grid::TrackListValue;
    pub(crate) use style::values::generics::grid::TrackSize;
    pub(crate) use style::values::generics::length::GenericLengthPercentageOrAuto;
    pub(crate) use style::values::generics::length::GenericLengthPercentageOrNormal;
    pub(crate) use style::values::generics::length::GenericMaxSize;
    pub(crate) use style::values::generics::length::GenericSize;
    pub(crate) use style::values::generics::position::PreferredRatio;
    pub(crate) use style::values::generics::NonNegative;
    pub(crate) use style::values::specified::align::AlignFlags;
    pub(crate) use style::values::specified::align::ContentDistribution;
    pub(crate) use style::values::specified::box_::Display;
    pub(crate) use style::values::specified::box_::DisplayInside;
    pub(crate) use style::values::specified::box_::DisplayOutside;
    pub(crate) use style::values::specified::box_::Overflow;
    pub(crate) use style::values::specified::GenericGridTemplateComponent;
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
        stylo::DisplayInside::Grid => taffy::Display::Grid,
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

pub(crate) fn grid_auto_flow(input: stylo::GridAutoFlow) -> taffy::GridAutoFlow {
    let is_row = input.contains(stylo::GridAutoFlow::ROW);
    let is_dense = input.contains(stylo::GridAutoFlow::DENSE);

    match (is_row, is_dense) {
        (true, false) => taffy::GridAutoFlow::Row,
        (true, true) => taffy::GridAutoFlow::RowDense,
        (false, false) => taffy::GridAutoFlow::Column,
        (false, true) => taffy::GridAutoFlow::ColumnDense,
    }
}

pub(crate) fn grid_line(input: &stylo::GridLine) -> taffy::GridPlacement {
    if input.is_auto() {
        taffy::GridPlacement::Auto
    } else if input.is_span {
        taffy::style_helpers::span(input.line_num.try_into().unwrap())
    } else {
        taffy::style_helpers::line(input.line_num.try_into().unwrap())
    }
}

pub(crate) fn grid_template_tracks(
    input: &stylo::GridTemplateComponent,
) -> Vec<taffy::TrackSizingFunction> {
    match input {
        stylo::GenericGridTemplateComponent::None => Vec::new(),
        stylo::GenericGridTemplateComponent::TrackList(list) => list
            .values
            .iter()
            .map(|track| match track {
                stylo::TrackListValue::TrackSize(size) => {
                    taffy::TrackSizingFunction::Single(track_size(size))
                }
                stylo::TrackListValue::TrackRepeat(repeat) => taffy::TrackSizingFunction::Repeat(
                    track_repeat(repeat.count),
                    repeat.track_sizes.iter().map(track_size).collect(),
                ),
            })
            .collect(),

        // TODO: Implement subgrid and masonry
        stylo::GenericGridTemplateComponent::Subgrid(_) => Vec::new(),
        stylo::GenericGridTemplateComponent::Masonry => Vec::new(),
    }
}

pub(crate) fn grid_auto_tracks(
    input: &stylo::ImplicitGridTracks,
) -> Vec<taffy::NonRepeatedTrackSizingFunction> {
    input.0.iter().map(track_size).collect()
}

pub(crate) fn track_repeat(input: stylo::RepeatCount<i32>) -> taffy::GridTrackRepetition {
    match input {
        stylo::RepeatCount::Number(val) => {
            taffy::GridTrackRepetition::Count(val.try_into().unwrap())
        }
        stylo::RepeatCount::AutoFill => taffy::GridTrackRepetition::AutoFill,
        stylo::RepeatCount::AutoFit => taffy::GridTrackRepetition::AutoFit,
    }
}

pub(crate) fn track_size(
    input: &stylo::TrackSize<stylo::LengthPercentage>,
) -> taffy::NonRepeatedTrackSizingFunction {
    match input {
        stylo::TrackSize::Breadth(breadth) => taffy::MinMax {
            min: min_track(breadth),
            max: max_track(breadth),
        },
        stylo::TrackSize::Minmax(min, max) => taffy::MinMax {
            min: min_track(min),
            max: max_track(max),
        },
        stylo::TrackSize::FitContent(limit) => taffy::MinMax {
            min: taffy::MinTrackSizingFunction::Auto,
            max: taffy::MaxTrackSizingFunction::FitContent(match limit {
                stylo::TrackBreadth::Breadth(lp) => length_percentage(lp),

                // Are these valid? Taffy doesn't support this in any case
                stylo::TrackBreadth::Fr(_) => unreachable!(),
                stylo::TrackBreadth::Auto => unreachable!(),
                stylo::TrackBreadth::MinContent => unreachable!(),
                stylo::TrackBreadth::MaxContent => unreachable!(),
            }),
        },
    }
}

pub(crate) fn min_track(
    input: &stylo::TrackBreadth<stylo::LengthPercentage>,
) -> taffy::MinTrackSizingFunction {
    match input {
        stylo::TrackBreadth::Breadth(lp) => {
            taffy::MinTrackSizingFunction::Fixed(length_percentage(lp))
        }
        stylo::TrackBreadth::Fr(_) => taffy::MinTrackSizingFunction::Auto,
        stylo::TrackBreadth::Auto => taffy::MinTrackSizingFunction::Auto,
        stylo::TrackBreadth::MinContent => taffy::MinTrackSizingFunction::MinContent,
        stylo::TrackBreadth::MaxContent => taffy::MinTrackSizingFunction::MaxContent,
    }
}

pub(crate) fn max_track(
    input: &stylo::TrackBreadth<stylo::LengthPercentage>,
) -> taffy::MaxTrackSizingFunction {
    match input {
        stylo::TrackBreadth::Breadth(lp) => {
            taffy::MaxTrackSizingFunction::Fixed(length_percentage(lp))
        }
        stylo::TrackBreadth::Fr(val) => taffy::MaxTrackSizingFunction::Fraction(*val),
        stylo::TrackBreadth::Auto => taffy::MaxTrackSizingFunction::Auto,
        stylo::TrackBreadth::MinContent => taffy::MaxTrackSizingFunction::MinContent,
        stylo::TrackBreadth::MaxContent => taffy::MaxTrackSizingFunction::MaxContent,
    }
}

pub(crate) fn content_alignment(input: stylo::ContentDistribution) -> Option<taffy::AlignContent> {
    match input.primary().value() {
        stylo::AlignFlags::NORMAL => None,
        stylo::AlignFlags::AUTO => None,
        stylo::AlignFlags::START => Some(taffy::AlignContent::Start),
        stylo::AlignFlags::END => Some(taffy::AlignContent::End),
        stylo::AlignFlags::FLEX_START => Some(taffy::AlignContent::FlexStart),
        stylo::AlignFlags::STRETCH => Some(taffy::AlignContent::Stretch),
        stylo::AlignFlags::FLEX_END => Some(taffy::AlignContent::FlexEnd),
        stylo::AlignFlags::CENTER => Some(taffy::AlignContent::Center),
        stylo::AlignFlags::SPACE_BETWEEN => Some(taffy::AlignContent::SpaceBetween),
        stylo::AlignFlags::SPACE_AROUND => Some(taffy::AlignContent::SpaceAround),
        stylo::AlignFlags::SPACE_EVENLY => Some(taffy::AlignContent::SpaceEvenly),
        // Should never be hit. But no real reason to panic here.
        _ => None,
    }
}

pub(crate) fn item_alignment(input: stylo::AlignFlags) -> Option<taffy::AlignItems> {
    match input.value() {
        stylo::AlignFlags::NORMAL => None,
        stylo::AlignFlags::AUTO => None,
        stylo::AlignFlags::STRETCH => Some(taffy::AlignItems::Stretch),
        stylo::AlignFlags::FLEX_START => Some(taffy::AlignItems::FlexStart),
        stylo::AlignFlags::FLEX_END => Some(taffy::AlignItems::FlexEnd),
        stylo::AlignFlags::START => Some(taffy::AlignItems::Start),
        stylo::AlignFlags::END => Some(taffy::AlignItems::End),
        stylo::AlignFlags::CENTER => Some(taffy::AlignItems::Center),
        stylo::AlignFlags::BASELINE => Some(taffy::AlignItems::Baseline),
        // Should never be hit. But no real reason to panic here.
        _ => None,
    }
}
