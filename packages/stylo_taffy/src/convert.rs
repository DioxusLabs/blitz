//! Conversion functions from Stylo computed style types to Taffy equivalents

/// Private module of type aliases so we can refer to stylo types with nicer names
pub(crate) mod stylo {
    pub(crate) use style::Atom;
    pub(crate) use style::properties::ComputedValues;
    pub(crate) use style::properties::generated::longhands::box_sizing::computed_value::T as BoxSizing;
    pub(crate) use style::properties::generated::longhands::direction::computed_value::T as Direction;
    pub(crate) use style::properties::longhands::aspect_ratio::computed_value::T as AspectRatio;
    pub(crate) use style::properties::longhands::position::computed_value::T as Position;
    pub(crate) use style::values::computed::length_percentage::CalcLengthPercentage;
    pub(crate) use style::values::computed::length_percentage::Unpacked as UnpackedLengthPercentage;
    pub(crate) use style::values::computed::{BorderSideWidth, LengthPercentage, Percentage};
    pub(crate) use style::values::generics::NonNegative;
    pub(crate) use style::values::generics::length::{
        GenericLengthPercentageOrNormal, GenericMargin, GenericMaxSize, GenericSize,
    };
    pub(crate) use style::values::generics::position::{Inset as GenericInset, PreferredRatio};
    pub(crate) use style::values::specified::align::{AlignFlags, ContentDistribution};
    pub(crate) use style::values::specified::border::BorderStyle;
    pub(crate) use style::values::specified::box_::{
        Display, DisplayInside, DisplayOutside, Overflow,
    };
    pub(crate) use style::values::specified::position::GridTemplateAreas;
    pub(crate) use style::values::specified::position::NamedArea;
    pub(crate) use style_atoms::atom;
    pub(crate) type MarginVal = GenericMargin<LengthPercentage>;
    pub(crate) type InsetVal = GenericInset<Percentage, LengthPercentage>;
    pub(crate) type Size = GenericSize<NonNegative<LengthPercentage>>;
    pub(crate) type MaxSize = GenericMaxSize<NonNegative<LengthPercentage>>;

    pub(crate) type Gap = GenericLengthPercentageOrNormal<NonNegative<LengthPercentage>>;

    #[cfg(feature = "floats")]
    pub(crate) use style::values::computed::{Clear, Float};

    #[cfg(feature = "flexbox")]
    pub(crate) use style::{
        computed_values::{flex_direction::T as FlexDirection, flex_wrap::T as FlexWrap},
        values::generics::flex::GenericFlexBasis,
    };
    #[cfg(feature = "flexbox")]
    pub(crate) type FlexBasis = GenericFlexBasis<Size>;

    #[cfg(feature = "block")]
    pub(crate) use style::values::computed::text::TextAlign;
    #[cfg(feature = "grid")]
    pub(crate) use style::{
        computed_values::grid_auto_flow::T as GridAutoFlow,
        values::{
            computed::{GridLine, GridTemplateComponent, ImplicitGridTracks},
            generics::grid::{RepeatCount, TrackBreadth, TrackListValue, TrackSize},
            specified::GenericGridTemplateComponent,
        },
    };
}

use stylo::Atom;
use taffy::CompactLength;
use taffy::style_helpers::*;

/// Helper macro for logging unsupported CSS values that fall back to defaults.
/// Only logs when the `tracing` feature is enabled.
#[cfg(feature = "tracing")]
macro_rules! log_fallback {
    ($value:expr, $to:expr) => {
        tracing::debug!(
            "CSS value '{}' not fully supported, falling back to '{}'",
            $value,
            $to
        )
    };
}

#[cfg(not(feature = "tracing"))]
macro_rules! log_fallback {
    ($value:expr, $to:expr) => {
        // Consume args so callers don't trip `unused_variables` on the
        // bindings they pass in (e.g. `unsupported => { log_fallback!(...) }`).
        let _ = (&$value, &$to);
    };
}

/// Converts a Stylo LengthPercentage to a Taffy LengthPercentage.
///
/// # Safety
///
/// This function uses `unsafe` when converting calc() values. The pointer
/// casting is safe because:
/// - The pointer comes from Stylo's validated computed values
/// - Taffy's `from_raw` is designed to accept these specific pointer types
/// - The calc value is guaranteed to be valid by Stylo's type system
#[inline]
pub fn length_percentage(val: &stylo::LengthPercentage) -> taffy::LengthPercentage {
    match val.unpack() {
        stylo::UnpackedLengthPercentage::Calc(calc_ptr) => {
            let val =
                CompactLength::calc(calc_ptr as *const stylo::CalcLengthPercentage as *const ());
            // SAFETY: calc_ptr is a valid pointer from Stylo's computed values
            // and Taffy's from_raw expects this specific format
            unsafe { taffy::LengthPercentage::from_raw(val) }
        }
        stylo::UnpackedLengthPercentage::Length(len) => length(len.px()),
        stylo::UnpackedLengthPercentage::Percentage(percentage) => percent(percentage.0),
    }
}

#[inline]
pub fn dimension(val: &stylo::Size) -> taffy::Dimension {
    match val {
        stylo::Size::LengthPercentage(val) => length_percentage(&val.0).into(),
        stylo::Size::Auto => taffy::Dimension::AUTO,

        // TODO: implement other values in Taffy
        stylo::Size::MaxContent => {
            log_fallback!("max-content", "AUTO");
            taffy::Dimension::AUTO
        }
        stylo::Size::MinContent => {
            log_fallback!("min-content", "AUTO");
            taffy::Dimension::AUTO
        }
        stylo::Size::FitContent => {
            log_fallback!("fit-content", "AUTO");
            taffy::Dimension::AUTO
        }
        stylo::Size::FitContentFunction(_) => {
            log_fallback!("fit-content()", "AUTO");
            taffy::Dimension::AUTO
        }
        stylo::Size::Stretch => {
            log_fallback!("stretch", "AUTO");
            taffy::Dimension::AUTO
        }
        stylo::Size::WebkitFillAvailable => {
            log_fallback!("-webkit-fill-available", "AUTO");
            taffy::Dimension::AUTO
        }

        // Anchor positioning not yet supported, falling back to AUTO
        stylo::Size::AnchorSizeFunction(_) => {
            log_fallback!("anchor-size()", "AUTO");
            taffy::Dimension::AUTO
        }
        stylo::Size::AnchorContainingCalcFunction(_) => {
            log_fallback!("anchor()", "AUTO");
            taffy::Dimension::AUTO
        }
    }
}

#[inline]
pub fn max_size_dimension(val: &stylo::MaxSize) -> taffy::Dimension {
    match val {
        stylo::MaxSize::LengthPercentage(val) => length_percentage(&val.0).into(),
        stylo::MaxSize::None => taffy::Dimension::AUTO,

        // TODO: implement other values in Taffy
        stylo::MaxSize::MaxContent => {
            log_fallback!("max-content (max-size)", "AUTO");
            taffy::Dimension::AUTO
        }
        stylo::MaxSize::MinContent => {
            log_fallback!("min-content (max-size)", "AUTO");
            taffy::Dimension::AUTO
        }
        stylo::MaxSize::FitContent => {
            log_fallback!("fit-content (max-size)", "AUTO");
            taffy::Dimension::AUTO
        }
        stylo::MaxSize::FitContentFunction(_) => {
            log_fallback!("fit-content() (max-size)", "AUTO");
            taffy::Dimension::AUTO
        }
        stylo::MaxSize::Stretch => {
            log_fallback!("stretch (max-size)", "AUTO");
            taffy::Dimension::AUTO
        }
        stylo::MaxSize::WebkitFillAvailable => {
            log_fallback!("-webkit-fill-available (max-size)", "AUTO");
            taffy::Dimension::AUTO
        }

        // Anchor positioning not yet supported, falling back to AUTO
        stylo::MaxSize::AnchorSizeFunction(_) => {
            log_fallback!("anchor-size() (max-size)", "AUTO");
            taffy::Dimension::AUTO
        }
        stylo::MaxSize::AnchorContainingCalcFunction(_) => {
            log_fallback!("anchor() (max-size)", "AUTO");
            taffy::Dimension::AUTO
        }
    }
}

#[inline]
pub fn margin(val: &stylo::MarginVal) -> taffy::LengthPercentageAuto {
    match val {
        stylo::MarginVal::Auto => taffy::LengthPercentageAuto::AUTO,
        stylo::MarginVal::LengthPercentage(val) => length_percentage(val).into(),

        // Anchor positioning not yet supported, falling back to AUTO
        stylo::MarginVal::AnchorSizeFunction(_) => taffy::LengthPercentageAuto::AUTO,
        stylo::MarginVal::AnchorContainingCalcFunction(_) => taffy::LengthPercentageAuto::AUTO,
    }
}

#[inline]
pub fn border(
    width: &stylo::BorderSideWidth,
    style: stylo::BorderStyle,
) -> taffy::LengthPercentage {
    if style.none_or_hidden() {
        return taffy::style_helpers::zero();
    }
    taffy::style_helpers::length(width.0.to_f32_px())
}

#[inline]
pub fn inset(val: &stylo::InsetVal) -> taffy::LengthPercentageAuto {
    match val {
        stylo::InsetVal::Auto => taffy::LengthPercentageAuto::AUTO,
        stylo::InsetVal::LengthPercentage(val) => length_percentage(val).into(),

        // Anchor positioning not yet supported, falling back to AUTO
        stylo::InsetVal::AnchorSizeFunction(_) => taffy::LengthPercentageAuto::AUTO,
        stylo::InsetVal::AnchorFunction(_) => taffy::LengthPercentageAuto::AUTO,
        stylo::InsetVal::AnchorContainingCalcFunction(_) => taffy::LengthPercentageAuto::AUTO,
    }
}

#[inline]
pub fn is_block(input: stylo::Display) -> bool {
    matches!(input.outside(), stylo::DisplayOutside::Block)
        && matches!(
            input.inside(),
            stylo::DisplayInside::Flow | stylo::DisplayInside::FlowRoot
        )
}

#[inline]
pub fn is_table(input: stylo::Display) -> bool {
    matches!(input.inside(), stylo::DisplayInside::Table)
}

#[inline]
pub fn display(input: stylo::Display) -> taffy::Display {
    let mut display = match input.inside() {
        stylo::DisplayInside::None => taffy::Display::None,
        #[cfg(feature = "flexbox")]
        stylo::DisplayInside::Flex => taffy::Display::Flex,
        #[cfg(feature = "grid")]
        stylo::DisplayInside::Grid => taffy::Display::Grid,
        #[cfg(feature = "block")]
        stylo::DisplayInside::Flow => taffy::Display::Block,
        #[cfg(feature = "block")]
        stylo::DisplayInside::FlowRoot => taffy::Display::Block,
        #[cfg(feature = "block")]
        stylo::DisplayInside::TableCell => taffy::Display::Block,
        // TODO: Support display:contents in Taffy
        // TODO: Support table layout in Taffy
        #[cfg(feature = "grid")]
        stylo::DisplayInside::Table => taffy::Display::Grid,
        unsupported => {
            log_fallback!(&format!("display:{:?}", unsupported), "DEFAULT");
            taffy::Display::DEFAULT
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

#[inline]
pub fn box_generation_mode(input: stylo::Display) -> taffy::BoxGenerationMode {
    match input.inside() {
        stylo::DisplayInside::None => taffy::BoxGenerationMode::None,
        // stylo::DisplayInside::Contents => display = taffy::BoxGenerationMode::Contents,
        _ => taffy::BoxGenerationMode::Normal,
    }
}

#[inline]
pub fn box_sizing(input: stylo::BoxSizing) -> taffy::BoxSizing {
    match input {
        stylo::BoxSizing::BorderBox => taffy::BoxSizing::BorderBox,
        stylo::BoxSizing::ContentBox => taffy::BoxSizing::ContentBox,
    }
}

/// Converts Stylo position to Taffy position.
///
/// # Limitations
///
/// Taffy has limited position support compared to CSS:
/// - `position: static` is treated as `relative` because Taffy doesn't distinguish them
/// - `position: fixed` is treated as `absolute` (viewport positioning not supported)
/// - `position: sticky` is treated as `relative` (sticky behavior not implemented)
///
/// These limitations mean fixed elements won't stay in viewport during scroll,
/// and sticky elements won't stick to their container during scroll.
#[inline]
pub fn position(input: stylo::Position) -> taffy::Position {
    match input {
        stylo::Position::Relative => taffy::Position::Relative,
        stylo::Position::Static => {
            log_fallback!("position:static", "Relative");
            taffy::Position::Relative
        }
        stylo::Position::Absolute => taffy::Position::Absolute,
        stylo::Position::Fixed => {
            log_fallback!("position:fixed", "Absolute");
            taffy::Position::Absolute
        }
        stylo::Position::Sticky => {
            log_fallback!("position:sticky", "Relative");
            taffy::Position::Relative
        }
    }
}

/// Converts Stylo overflow to Taffy overflow.
///
/// # Limitations
///
/// `overflow: auto` falls back to `scroll` because Taffy doesn't have an `Auto` variant.
/// This means scrollbars will always be shown for overflow:auto containers,
/// rather than only appearing when content overflows.
#[inline]
pub fn overflow(input: stylo::Overflow) -> taffy::Overflow {
    match input {
        stylo::Overflow::Visible => taffy::Overflow::Visible,
        stylo::Overflow::Clip => taffy::Overflow::Clip,
        stylo::Overflow::Hidden => taffy::Overflow::Hidden,
        stylo::Overflow::Scroll => taffy::Overflow::Scroll,
        stylo::Overflow::Auto => {
            log_fallback!("overflow:auto", "Scroll");
            taffy::Overflow::Scroll
        }
    }
}

/// Converts Stylo aspect ratio to an optional f32.
///
/// Returns `None` for `aspect-ratio: auto` or if the denominator is zero
/// (which shouldn't happen in valid CSS but is handled defensively).
#[inline]
pub fn direction(input: stylo::Direction) -> taffy::Direction {
    match input {
        stylo::Direction::Ltr => taffy::Direction::Ltr,
        stylo::Direction::Rtl => taffy::Direction::Rtl,
    }
}

#[inline]
pub fn aspect_ratio(input: stylo::AspectRatio) -> Option<f32> {
    match input.ratio {
        stylo::PreferredRatio::None => None,
        stylo::PreferredRatio::Ratio(val) => {
            let denominator = val.1.0;
            if denominator == 0.0 {
                log_fallback!("aspect-ratio with zero denominator", "None");
                None
            } else {
                Some(val.0.0 / denominator)
            }
        }
    }
}

#[inline]
pub fn content_alignment(input: stylo::ContentDistribution) -> Option<taffy::AlignContent> {
    match input.primary().value() {
        stylo::AlignFlags::NORMAL => None,
        stylo::AlignFlags::AUTO => None,
        stylo::AlignFlags::START => Some(taffy::AlignContent::Start),
        stylo::AlignFlags::END => Some(taffy::AlignContent::End),
        stylo::AlignFlags::LEFT => Some(taffy::AlignContent::Start),
        stylo::AlignFlags::RIGHT => Some(taffy::AlignContent::End),
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

#[inline]
pub fn item_alignment(input: stylo::AlignFlags) -> Option<taffy::AlignItems> {
    match input.value() {
        stylo::AlignFlags::AUTO => None,
        stylo::AlignFlags::NORMAL => Some(taffy::AlignItems::Stretch),
        stylo::AlignFlags::STRETCH => Some(taffy::AlignItems::Stretch),
        stylo::AlignFlags::FLEX_START => Some(taffy::AlignItems::FlexStart),
        stylo::AlignFlags::FLEX_END => Some(taffy::AlignItems::FlexEnd),
        stylo::AlignFlags::SELF_START => Some(taffy::AlignItems::Start),
        stylo::AlignFlags::SELF_END => Some(taffy::AlignItems::End),
        stylo::AlignFlags::START => Some(taffy::AlignItems::Start),
        stylo::AlignFlags::END => Some(taffy::AlignItems::End),
        stylo::AlignFlags::LEFT => Some(taffy::AlignItems::Start),
        stylo::AlignFlags::RIGHT => Some(taffy::AlignItems::End),
        stylo::AlignFlags::CENTER => Some(taffy::AlignItems::Center),
        stylo::AlignFlags::BASELINE => Some(taffy::AlignItems::Baseline),
        // Should never be hit. But no real reason to panic here.
        _ => None,
    }
}

#[inline]
pub fn gap(input: &stylo::Gap) -> taffy::LengthPercentage {
    match input {
        // For Flexbox and CSS Grid the "normal" value is 0px. This may need to be updated
        // if we ever implement multi-column layout.
        stylo::Gap::Normal => taffy::LengthPercentage::ZERO,
        stylo::Gap::LengthPercentage(val) => length_percentage(&val.0),
    }
}

#[inline]
#[cfg(feature = "block")]
pub(crate) fn text_align(input: stylo::TextAlign) -> taffy::TextAlign {
    match input {
        stylo::TextAlign::MozLeft => taffy::TextAlign::LegacyLeft,
        stylo::TextAlign::MozRight => taffy::TextAlign::LegacyRight,
        stylo::TextAlign::MozCenter => taffy::TextAlign::LegacyCenter,
        _ => taffy::TextAlign::Auto,
    }
}

#[inline]
#[cfg(feature = "flexbox")]
pub fn flex_basis(input: &stylo::FlexBasis) -> taffy::Dimension {
    // TODO: Support flex-basis: content in Taffy
    match input {
        stylo::FlexBasis::Content => taffy::Dimension::AUTO,
        stylo::FlexBasis::Size(size) => dimension(size),
    }
}

#[inline]
#[cfg(feature = "flexbox")]
pub fn flex_direction(input: stylo::FlexDirection) -> taffy::FlexDirection {
    match input {
        stylo::FlexDirection::Row => taffy::FlexDirection::Row,
        stylo::FlexDirection::RowReverse => taffy::FlexDirection::RowReverse,
        stylo::FlexDirection::Column => taffy::FlexDirection::Column,
        stylo::FlexDirection::ColumnReverse => taffy::FlexDirection::ColumnReverse,
    }
}

#[inline]
#[cfg(feature = "flexbox")]
pub fn flex_wrap(input: stylo::FlexWrap) -> taffy::FlexWrap {
    match input {
        stylo::FlexWrap::Wrap => taffy::FlexWrap::Wrap,
        stylo::FlexWrap::WrapReverse => taffy::FlexWrap::WrapReverse,
        stylo::FlexWrap::Nowrap => taffy::FlexWrap::NoWrap,
    }
}

#[inline]
#[cfg(feature = "floats")]
pub fn float(input: stylo::Float) -> taffy::Float {
    match input {
        stylo::Float::Left => taffy::Float::Left,
        stylo::Float::Right => taffy::Float::Right,
        stylo::Float::None => taffy::Float::None,

        stylo::Float::InlineStart => taffy::Float::Left,
        stylo::Float::InlineEnd => taffy::Float::Right,
    }
}

#[inline]
#[cfg(feature = "floats")]
pub fn clear(input: stylo::Clear) -> taffy::Clear {
    match input {
        stylo::Clear::Left => taffy::Clear::Left,
        stylo::Clear::Right => taffy::Clear::Right,
        stylo::Clear::Both => taffy::Clear::Both,
        stylo::Clear::None => taffy::Clear::None,

        stylo::Clear::InlineStart => taffy::Clear::Left,
        stylo::Clear::InlineEnd => taffy::Clear::Right,
    }
}

// CSS Grid styles
// ===============

#[inline]
#[cfg(feature = "grid")]
pub fn grid_auto_flow(input: stylo::GridAutoFlow) -> taffy::GridAutoFlow {
    let is_row = input.contains(stylo::GridAutoFlow::ROW);
    let is_dense = input.contains(stylo::GridAutoFlow::DENSE);

    match (is_row, is_dense) {
        (true, false) => taffy::GridAutoFlow::Row,
        (true, true) => taffy::GridAutoFlow::RowDense,
        (false, false) => taffy::GridAutoFlow::Column,
        (false, true) => taffy::GridAutoFlow::ColumnDense,
    }
}

/// Converts a Stylo grid line to a Taffy grid placement.
///
/// # Edge Case Handling
///
/// - Line numbers that overflow i16/u16 are clamped to their respective max values
/// - Negative line numbers are preserved (valid in CSS grid)
/// - Zero line number without ident falls back to Auto
#[inline]
#[cfg(feature = "grid")]
pub fn grid_line(input: &stylo::GridLine) -> taffy::GridPlacement<Atom> {
    if input.is_auto() {
        taffy::GridPlacement::Auto
    } else if input.is_span {
        // Clamp span count to valid u16 range
        let span_count = if input.line_num <= 0 {
            log_fallback!("grid span with non-positive count", "1");
            1u16
        } else {
            input.line_num.try_into().unwrap_or(u16::MAX)
        };

        if input.ident.0 != stylo::atom!("") {
            taffy::GridPlacement::NamedSpan(input.ident.0.clone(), span_count)
        } else {
            taffy::GridPlacement::Span(span_count)
        }
    } else if input.ident.0 != stylo::atom!("") {
        // Named line with optional line number offset
        let line_num = input.line_num.try_into().unwrap_or(i16::MAX);
        taffy::GridPlacement::NamedLine(input.ident.0.clone(), line_num)
    } else if input.line_num != 0 {
        // Numeric line number only - clamp to valid i16 range
        let line_num = input.line_num.try_into().unwrap_or(i16::MAX);
        taffy::style_helpers::line(line_num)
    } else {
        taffy::GridPlacement::Auto
    }
}

#[inline]
#[cfg(feature = "grid")]
pub fn grid_template_tracks(
    input: &stylo::GridTemplateComponent,
) -> Vec<taffy::GridTemplateComponent<Atom>> {
    match input {
        stylo::GenericGridTemplateComponent::None => Vec::new(),
        stylo::GenericGridTemplateComponent::TrackList(list) => list
            .values
            .iter()
            .map(|track| match track {
                stylo::TrackListValue::TrackSize(size) => {
                    taffy::GridTemplateComponent::Single(track_size(size))
                }
                stylo::TrackListValue::TrackRepeat(repeat) => {
                    taffy::GridTemplateComponent::Repeat(taffy::GridTemplateRepetition {
                        count: track_repeat(repeat.count),
                        tracks: repeat.track_sizes.iter().map(track_size).collect(),
                        line_names: repeat
                            .line_names
                            .iter()
                            .map(|line_name_set| {
                                line_name_set
                                    .iter()
                                    .map(|ident| ident.0.clone())
                                    .collect::<Vec<_>>()
                            })
                            .collect::<Vec<_>>(),
                    })
                }
            })
            .collect(),

        // TODO: Implement subgrid and masonry
        stylo::GenericGridTemplateComponent::Subgrid(_) => Vec::new(),
        stylo::GenericGridTemplateComponent::Masonry => Vec::new(),
    }
}

#[inline]
#[cfg(feature = "grid")]
pub fn grid_template_line_names(
    input: &stylo::GridTemplateComponent,
) -> Option<crate::wrapper::StyloLineNameIter<'_>> {
    match input {
        stylo::GenericGridTemplateComponent::None => None,
        stylo::GenericGridTemplateComponent::TrackList(list) => {
            Some(crate::wrapper::StyloLineNameIter::new(&list.line_names))
        }

        // TODO: Implement subgrid and masonry
        stylo::GenericGridTemplateComponent::Subgrid(_) => None,
        stylo::GenericGridTemplateComponent::Masonry => None,
    }
}

#[inline]
#[cfg(feature = "grid")]
pub fn grid_template_area(input: &stylo::NamedArea) -> taffy::GridTemplateArea<Atom> {
    taffy::GridTemplateArea {
        name: input.name.clone(),
        row_start: input.rows.start as u16,
        row_end: input.rows.end as u16,
        column_start: input.columns.start as u16,
        column_end: input.columns.end as u16,
    }
}

#[inline]
#[cfg(feature = "grid")]
fn grid_template_areas(input: &stylo::GridTemplateAreas) -> Vec<taffy::GridTemplateArea<Atom>> {
    match input {
        stylo::GridTemplateAreas::None => Vec::new(),
        stylo::GridTemplateAreas::Areas(template_areas_arc) => {
            crate::wrapper::GridAreaWrapper(&template_areas_arc.0.areas)
                .into_iter()
                .collect()
        }
    }
}

#[inline]
#[cfg(feature = "grid")]
pub fn grid_auto_tracks(input: &stylo::ImplicitGridTracks) -> Vec<taffy::TrackSizingFunction> {
    input.0.iter().map(track_size).collect()
}

#[inline]
#[cfg(feature = "grid")]
pub fn track_repeat(input: stylo::RepeatCount<i32>) -> taffy::RepetitionCount {
    match input {
        stylo::RepeatCount::Number(val) => {
            taffy::RepetitionCount::Count(val.try_into().unwrap_or(u16::MAX))
        }
        stylo::RepeatCount::AutoFill => taffy::RepetitionCount::AutoFill,
        stylo::RepeatCount::AutoFit => taffy::RepetitionCount::AutoFit,
    }
}

#[inline]
#[cfg(feature = "grid")]
pub fn track_size(input: &stylo::TrackSize<stylo::LengthPercentage>) -> taffy::TrackSizingFunction {
    use taffy::MaxTrackSizingFunction;

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
            min: taffy::MinTrackSizingFunction::AUTO,
            max: match limit {
                stylo::TrackBreadth::Breadth(lp) => {
                    MaxTrackSizingFunction::fit_content(length_percentage(lp))
                }

                // These shouldn't occur in fit-content() per CSS spec, but handle gracefully
                stylo::TrackBreadth::Fr(_) => MaxTrackSizingFunction::AUTO,
                stylo::TrackBreadth::Auto => MaxTrackSizingFunction::AUTO,
                stylo::TrackBreadth::MinContent => MaxTrackSizingFunction::MIN_CONTENT,
                stylo::TrackBreadth::MaxContent => MaxTrackSizingFunction::MAX_CONTENT,
            },
        },
    }
}

#[inline]
#[cfg(feature = "grid")]
pub fn min_track(
    input: &stylo::TrackBreadth<stylo::LengthPercentage>,
) -> taffy::MinTrackSizingFunction {
    use taffy::prelude::*;
    match input {
        stylo::TrackBreadth::Breadth(lp) => {
            taffy::MinTrackSizingFunction::from(length_percentage(lp))
        }
        stylo::TrackBreadth::Fr(_) => taffy::MinTrackSizingFunction::AUTO,
        stylo::TrackBreadth::Auto => taffy::MinTrackSizingFunction::AUTO,
        stylo::TrackBreadth::MinContent => taffy::MinTrackSizingFunction::MIN_CONTENT,
        stylo::TrackBreadth::MaxContent => taffy::MinTrackSizingFunction::MAX_CONTENT,
    }
}

#[inline]
#[cfg(feature = "grid")]
pub fn max_track(
    input: &stylo::TrackBreadth<stylo::LengthPercentage>,
) -> taffy::MaxTrackSizingFunction {
    use taffy::prelude::*;

    match input {
        stylo::TrackBreadth::Breadth(lp) => {
            taffy::MaxTrackSizingFunction::from(length_percentage(lp))
        }
        stylo::TrackBreadth::Fr(val) => taffy::MaxTrackSizingFunction::from_fr(*val),
        stylo::TrackBreadth::Auto => taffy::MaxTrackSizingFunction::AUTO,
        stylo::TrackBreadth::MinContent => taffy::MaxTrackSizingFunction::MIN_CONTENT,
        stylo::TrackBreadth::MaxContent => taffy::MaxTrackSizingFunction::MAX_CONTENT,
    }
}

/// Eagerly convert an entire [`stylo::ComputedValues`] into a [`taffy::Style`]
pub fn to_taffy_style(style: &stylo::ComputedValues) -> taffy::Style<Atom> {
    let display = style.clone_display();
    let pos = style.get_position();
    let margin = style.get_margin();
    let padding = style.get_padding();
    let border = style.get_border();

    taffy::Style {
        dummy: core::marker::PhantomData,
        display: self::display(display),
        box_sizing: self::box_sizing(style.clone_box_sizing()),
        item_is_table: display.inside() == stylo::DisplayInside::Table,
        item_is_replaced: false,
        position: self::position(style.clone_position()),
        overflow: taffy::Point {
            x: self::overflow(style.clone_overflow_x()),
            y: self::overflow(style.clone_overflow_y()),
        },
        direction: self::direction(style.clone_direction()),
        scrollbar_width: 0.0,

        #[cfg(feature = "floats")]
        float: self::float(style.clone_float()),
        #[cfg(feature = "floats")]
        clear: self::clear(style.clone_clear()),

        size: taffy::Size {
            width: self::dimension(&pos.width),
            height: self::dimension(&pos.height),
        },
        min_size: taffy::Size {
            width: self::dimension(&pos.min_width),
            height: self::dimension(&pos.min_height),
        },
        max_size: taffy::Size {
            width: self::max_size_dimension(&pos.max_width),
            height: self::max_size_dimension(&pos.max_height),
        },
        aspect_ratio: self::aspect_ratio(pos.aspect_ratio),

        inset: taffy::Rect {
            left: self::inset(&pos.left),
            right: self::inset(&pos.right),
            top: self::inset(&pos.top),
            bottom: self::inset(&pos.bottom),
        },
        margin: taffy::Rect {
            left: self::margin(&margin.margin_left),
            right: self::margin(&margin.margin_right),
            top: self::margin(&margin.margin_top),
            bottom: self::margin(&margin.margin_bottom),
        },
        padding: taffy::Rect {
            left: self::length_percentage(&padding.padding_left.0),
            right: self::length_percentage(&padding.padding_right.0),
            top: self::length_percentage(&padding.padding_top.0),
            bottom: self::length_percentage(&padding.padding_bottom.0),
        },
        border: taffy::Rect {
            left: self::border(&border.border_left_width, border.border_left_style),
            right: self::border(&border.border_right_width, border.border_right_style),
            top: self::border(&border.border_top_width, border.border_top_style),
            bottom: self::border(&border.border_bottom_width, border.border_bottom_style),
        },

        // Gap
        #[cfg(any(feature = "flexbox", feature = "grid"))]
        gap: taffy::Size {
            width: self::gap(&pos.column_gap),
            height: self::gap(&pos.row_gap),
        },

        // Alignment
        #[cfg(any(feature = "flexbox", feature = "grid"))]
        align_content: self::content_alignment(pos.align_content),
        #[cfg(any(feature = "flexbox", feature = "grid"))]
        justify_content: self::content_alignment(pos.justify_content),
        #[cfg(any(feature = "flexbox", feature = "grid"))]
        align_items: self::item_alignment(pos.align_items.0),
        #[cfg(any(feature = "flexbox", feature = "grid"))]
        align_self: self::item_alignment(pos.align_self.0),
        #[cfg(feature = "grid")]
        justify_items: self::item_alignment((pos.justify_items.computed.0).0),
        #[cfg(feature = "grid")]
        justify_self: self::item_alignment(pos.justify_self.0),
        #[cfg(feature = "block")]
        text_align: self::text_align(style.clone_text_align()),

        // Flexbox
        #[cfg(feature = "flexbox")]
        flex_direction: self::flex_direction(pos.flex_direction),
        #[cfg(feature = "flexbox")]
        flex_wrap: self::flex_wrap(pos.flex_wrap),
        #[cfg(feature = "flexbox")]
        flex_grow: pos.flex_grow.0,
        #[cfg(feature = "flexbox")]
        flex_shrink: pos.flex_shrink.0,
        #[cfg(feature = "flexbox")]
        flex_basis: self::flex_basis(&pos.flex_basis),

        // Grid
        #[cfg(feature = "grid")]
        grid_auto_flow: self::grid_auto_flow(pos.grid_auto_flow),
        #[cfg(feature = "grid")]
        grid_template_rows: self::grid_template_tracks(&pos.grid_template_rows),
        #[cfg(feature = "grid")]
        grid_template_columns: self::grid_template_tracks(&pos.grid_template_columns),
        #[cfg(feature = "grid")]
        grid_template_row_names: match self::grid_template_line_names(&pos.grid_template_rows) {
            Some(iter) => iter
                .map(|line_name_set| line_name_set.cloned().collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            None => Vec::new(),
        },
        #[cfg(feature = "grid")]
        grid_template_column_names: match self::grid_template_line_names(&pos.grid_template_columns)
        {
            Some(iter) => iter
                .map(|line_name_set| line_name_set.cloned().collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            None => Vec::new(),
        },
        #[cfg(feature = "grid")]
        grid_template_areas: self::grid_template_areas(&pos.grid_template_areas),
        #[cfg(feature = "grid")]
        grid_auto_rows: self::grid_auto_tracks(&pos.grid_auto_rows),
        #[cfg(feature = "grid")]
        grid_auto_columns: self::grid_auto_tracks(&pos.grid_auto_columns),
        #[cfg(feature = "grid")]
        grid_row: taffy::Line {
            start: self::grid_line(&pos.grid_row_start),
            end: self::grid_line(&pos.grid_row_end),
        },
        #[cfg(feature = "grid")]
        grid_column: taffy::Line {
            start: self::grid_line(&pos.grid_column_start),
            end: self::grid_line(&pos.grid_column_end),
        },
    }
}
