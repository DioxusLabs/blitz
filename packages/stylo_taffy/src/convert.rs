//! Conversion functions from Stylo computed style types to Taffy equivalents

/// Private module of type aliases so we can refer to stylo types with nicer names
pub(crate) mod stylo {
    pub(crate) use style::Atom;
    pub(crate) use style::logical_geometry::WritingMode;
    pub(crate) use style::properties::ComputedValues;
    pub(crate) use style::properties::generated::longhands::box_sizing::computed_value::T as BoxSizing;
    pub(crate) use style::properties::generated::longhands::direction::computed_value::T as Direction;
    pub(crate) use style::properties::longhands::aspect_ratio::computed_value::T as AspectRatio;
    pub(crate) use style::properties::longhands::position::computed_value::T as Position;
    pub(crate) use style::values::computed::CSSPixelLength;
    pub(crate) use style::values::computed::length_percentage::CalcLengthPercentage;
    pub(crate) use style::values::computed::length_percentage::Unpacked as UnpackedLengthPercentage;
    pub(crate) use style::values::computed::{
        BorderSideWidth, Contain, LengthPercentage, Percentage,
    };
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
    #[cfg(feature = "block")]
    pub(crate) use style::values::computed::{AlignmentBaseline, BaselineShift};
    #[cfg(feature = "block")]
    pub(crate) use style::values::generics::box_::BaselineShiftKeyword;
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

#[inline]
#[cfg(any(feature = "flexbox", feature = "grid"))]
pub(crate) fn saturating_i16(input: i32) -> i16 {
    input.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

#[inline]
#[cfg(any(feature = "flexbox", feature = "grid"))]
/// Clamps unsigned counts to the nonnegative i16 range.
pub(crate) fn saturating_u16<T: Ord + From<u16> + TryInto<i32>>(input: T) -> u16 {
    let input = input.max(u16::MIN.into()).try_into().unwrap_or(i32::MAX);
    saturating_i16(input) as u16
}

#[inline]
pub fn length_percentage(val: &stylo::LengthPercentage) -> taffy::LengthPercentage {
    match val.unpack() {
        stylo::UnpackedLengthPercentage::Calc(calc_ptr) => {
            let val =
                CompactLength::calc(calc_ptr as *const stylo::CalcLengthPercentage as *const ());
            // SAFETY: calc is a valid value for LengthPercentage
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

        stylo::Size::MaxContent => taffy::Dimension::max_content(),
        stylo::Size::MinContent => taffy::Dimension::min_content(),
        stylo::Size::FitContent => taffy::Dimension::fit_content(),
        stylo::Size::FitContentFunction(val) => match val.0.unpack() {
            stylo::UnpackedLengthPercentage::Length(len) => {
                taffy::Dimension::fit_content_px(len.px())
            }
            stylo::UnpackedLengthPercentage::Percentage(percentage) => {
                taffy::Dimension::fit_content_percent(percentage.0)
            }
            stylo::UnpackedLengthPercentage::Calc(calc_ptr) => taffy::Dimension::fit_content_calc(
                calc_ptr as *const stylo::CalcLengthPercentage as *const (),
            ),
        },

        stylo::Size::Stretch => taffy::Dimension::stretch(),
        stylo::Size::WebkitFillAvailable => taffy::Dimension::stretch(),

        // Anchor positioning will be flagged off for time being
        stylo::Size::AnchorSizeFunction(_) => unreachable!(),
        stylo::Size::AnchorContainingCalcFunction(_) => unreachable!(),
    }
}

#[inline]
pub fn min_size(val: &stylo::Size) -> taffy::LengthPercentageAuto {
    match val {
        stylo::Size::LengthPercentage(val) => length_percentage(&val.0).into(),
        stylo::Size::Auto => taffy::LengthPercentageAuto::AUTO,

        // Sizing keywords are not supported for min/max size properties in Taffy
        stylo::Size::MaxContent => taffy::LengthPercentageAuto::AUTO,
        stylo::Size::MinContent => taffy::LengthPercentageAuto::AUTO,
        stylo::Size::FitContent => taffy::LengthPercentageAuto::AUTO,
        stylo::Size::FitContentFunction(_) => taffy::LengthPercentageAuto::AUTO,
        stylo::Size::Stretch => taffy::LengthPercentageAuto::AUTO,
        stylo::Size::WebkitFillAvailable => taffy::LengthPercentageAuto::AUTO,

        // Anchor positioning will be flagged off for time being
        stylo::Size::AnchorSizeFunction(_) => unreachable!(),
        stylo::Size::AnchorContainingCalcFunction(_) => unreachable!(),
    }
}

#[inline]
pub fn max_size(val: &stylo::MaxSize) -> taffy::LengthPercentageAuto {
    match val {
        stylo::MaxSize::LengthPercentage(val) => length_percentage(&val.0).into(),
        stylo::MaxSize::None => taffy::LengthPercentageAuto::AUTO,

        // Sizing keywords are not supported for min/max size properties in Taffy
        stylo::MaxSize::MaxContent => taffy::LengthPercentageAuto::AUTO,
        stylo::MaxSize::MinContent => taffy::LengthPercentageAuto::AUTO,
        stylo::MaxSize::FitContent => taffy::LengthPercentageAuto::AUTO,
        stylo::MaxSize::FitContentFunction(_) => taffy::LengthPercentageAuto::AUTO,
        stylo::MaxSize::Stretch => taffy::LengthPercentageAuto::AUTO,
        stylo::MaxSize::WebkitFillAvailable => taffy::LengthPercentageAuto::AUTO,

        // Anchor positioning will be flagged off for time being
        stylo::MaxSize::AnchorSizeFunction(_) => unreachable!(),
        stylo::MaxSize::AnchorContainingCalcFunction(_) => unreachable!(),
    }
}

#[inline]
pub fn margin(val: &stylo::MarginVal) -> taffy::LengthPercentageAuto {
    match val {
        stylo::MarginVal::Auto => taffy::LengthPercentageAuto::AUTO,
        stylo::MarginVal::LengthPercentage(val) => length_percentage(val).into(),

        // Anchor positioning will be flagged off for time being
        stylo::MarginVal::AnchorSizeFunction(_) => unreachable!(),
        stylo::MarginVal::AnchorContainingCalcFunction(_) => unreachable!(),
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

        // Anchor positioning will be flagged off for time being
        stylo::InsetVal::AnchorSizeFunction(_) => unreachable!(),
        stylo::InsetVal::AnchorFunction(_) => unreachable!(),
        stylo::InsetVal::AnchorContainingCalcFunction(_) => unreachable!(),
    }
}

#[inline]
pub fn is_block(input: stylo::Display) -> bool {
    self::display(input) == taffy::Display::Block
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
        stylo::DisplayInside::FlowRoot => taffy::Display::FlowRoot,
        #[cfg(feature = "block")]
        stylo::DisplayInside::TableCell => taffy::Display::Block,
        // TODO: Support display:contents in Taffy
        // TODO: Support table layout in Taffy
        #[cfg(feature = "grid")]
        stylo::DisplayInside::Table => taffy::Display::Grid,
        _ => {
            // println!("FALLBACK {:?} {:?}", input.inside(), input.outside());
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

/// Convert the inset properties to a Taffy `Rect`. Insets have no effect on
/// `position: static` boxes, so they are reported as `auto` in that case.
#[inline]
/// Resolve a `calc()` length-percentage (as stored in a [`taffy::LengthPercentage`] created by
/// [`length_percentage`]) against `parent_size`
pub fn resolve_calc_value(calc_ptr: *const (), parent_size: f32) -> f32 {
    let calc = unsafe { &*(calc_ptr as *const stylo::CalcLengthPercentage) };
    calc.resolve(stylo::CSSPixelLength::new(parent_size)).px()
}

pub fn inset_rect(style: &stylo::ComputedValues) -> taffy::Rect<taffy::LengthPercentageAuto> {
    if style.get_box().position == stylo::Position::Static {
        return taffy::Rect::auto();
    }
    let pos = style.get_position();
    taffy::Rect {
        left: self::inset(&pos.left),
        right: self::inset(&pos.right),
        top: self::inset(&pos.top),
        bottom: self::inset(&pos.bottom),
    }
}

pub fn position(input: stylo::Position) -> taffy::Position {
    match input {
        stylo::Position::Static => taffy::Position::Static,
        stylo::Position::Relative => taffy::Position::Relative,
        stylo::Position::Absolute => taffy::Position::Absolute,
        stylo::Position::Fixed => taffy::Position::Fixed,
        stylo::Position::Sticky => taffy::Position::Sticky,
    }
}

/// Whether a style establishes a containing block for `position: fixed` (and therefore also
/// `position: absolute`) descendants, independently of its `position` value.
///
/// <https://developer.mozilla.org/en-US/docs/Web/CSS/CSS_positioned_layout/Containing_block#identifying_the_containing_block>
pub fn establishes_fixed_containing_block(style: &stylo::ComputedValues) -> bool {
    use style::values::computed::{Perspective, Rotate, Scale, Translate};
    use style::values::specified::box_::{Contain, ContainerType, WillChangeBits};

    let box_style = style.get_box();
    if !box_style.transform.0.is_empty()
        || !matches!(box_style.translate, Translate::None)
        || !matches!(box_style.rotate, Rotate::None)
        || !matches!(box_style.scale, Scale::None)
        || !matches!(box_style.perspective, Perspective::None)
    {
        return true;
    }
    if box_style.will_change.bits.intersects(
        WillChangeBits::TRANSFORM
            | WillChangeBits::PERSPECTIVE
            | WillChangeBits::FIXPOS_CB_NON_SVG
            | WillChangeBits::CONTAIN,
    ) {
        return true;
    }
    if box_style
        .contain
        .intersects(Contain::LAYOUT | Contain::PAINT)
    {
        return true;
    }
    if box_style
        .container_type
        .intersects(ContainerType::SIZE | ContainerType::INLINE_SIZE)
    {
        return true;
    }

    let effects = style.get_effects();
    !effects.filter.0.is_empty() || !effects.backdrop_filter.0.is_empty()
}

/// Whether a style establishes a containing block for `position: absolute` descendants even
/// when it is not positioned (e.g. `will-change: position`).
///
/// <https://drafts.csswg.org/css-will-change/#will-change>
pub fn establishes_absolute_containing_block(style: &stylo::ComputedValues) -> bool {
    use style::values::specified::box_::WillChangeBits;

    style
        .get_box()
        .will_change
        .bits
        .intersects(WillChangeBits::POSITION)
}

/// Which out-of-flow positions a style establishes a containing block for.
///
/// Positioned (non-`static`) elements are containing blocks for `absolute` boxes; elements that
/// establish a fixed containing block (transforms, filters, `will-change`, `contain`, ...) are
/// containing blocks for both `absolute` and `fixed` boxes.
pub fn containing_block_claims(style: &stylo::ComputedValues) -> taffy::ContainingBlockClaims {
    let is_positioned = style.get_box().position != stylo::Position::Static;
    let fixed = establishes_fixed_containing_block(style);
    taffy::ContainingBlockClaims {
        absolute: is_positioned || fixed || establishes_absolute_containing_block(style),
        fixed,
    }
}

#[inline]
pub fn overflow(input: stylo::Overflow) -> taffy::Overflow {
    match input {
        stylo::Overflow::Visible => taffy::Overflow::Visible,
        stylo::Overflow::Clip => taffy::Overflow::Clip,
        stylo::Overflow::Hidden => taffy::Overflow::Hidden,
        stylo::Overflow::Scroll => taffy::Overflow::Scroll,
        // TODO: Support Overflow::Auto in Taffy
        stylo::Overflow::Auto => taffy::Overflow::Scroll,
    }
}

#[inline]
pub fn contain(input: stylo::Contain, display: stylo::Display) -> taffy::Contain {
    // Layout and paint containment do not apply to non-atomic inline-level boxes
    // (https://drafts.csswg.org/css-contain-1/#containment-layout)
    if display.outside() == stylo::DisplayOutside::Inline
        && display.inside() == stylo::DisplayInside::Flow
    {
        return taffy::Contain::NONE;
    }
    let mut result = taffy::Contain::NONE;
    if input.contains(stylo::Contain::LAYOUT) {
        result |= taffy::Contain::LAYOUT;
    }
    if input.contains(stylo::Contain::PAINT) {
        result |= taffy::Contain::PAINT;
    }
    result
}

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
        // A degenerate ratio (either side zero) behaves as `auto`:
        // <https://drafts.csswg.org/css-sizing-4/#aspect-ratio>
        stylo::PreferredRatio::Ratio(val) if !val.is_degenerate() => Some(val.0.0 / val.1.0),
        _ => None,
    }
}

/// Convert `align-content`/`justify-content` for a container with the given `display`.
///
/// In a block container the whole in-flow content is a single alignment subject, and positional
/// keywords default to `safe` overflow alignment, only opted out of by an explicit `unsafe`
/// (<https://github.com/w3c/csswg-drafts/issues/10154>).
#[inline]
pub fn content_alignment(
    input: stylo::ContentDistribution,
    display: stylo::Display,
) -> taffy::AlignContent {
    let primary = input.primary();
    let mut align = match primary.value() {
        stylo::AlignFlags::NORMAL => return taffy::AlignContent::NORMAL,
        stylo::AlignFlags::AUTO => return taffy::AlignContent::NORMAL,
        stylo::AlignFlags::START => taffy::AlignContent::START,
        stylo::AlignFlags::END => taffy::AlignContent::END,
        stylo::AlignFlags::LEFT => taffy::AlignContent::START,
        stylo::AlignFlags::RIGHT => taffy::AlignContent::END,
        stylo::AlignFlags::FLEX_START => taffy::AlignContent::FLEX_START,
        stylo::AlignFlags::STRETCH => taffy::AlignContent::STRETCH,
        stylo::AlignFlags::FLEX_END => taffy::AlignContent::FLEX_END,
        stylo::AlignFlags::CENTER => taffy::AlignContent::CENTER,
        stylo::AlignFlags::SPACE_BETWEEN => taffy::AlignContent::SPACE_BETWEEN,
        stylo::AlignFlags::SPACE_AROUND => taffy::AlignContent::SPACE_AROUND,
        stylo::AlignFlags::SPACE_EVENLY => taffy::AlignContent::SPACE_EVENLY,
        // Baseline content-alignment is not supported: it falls back to start/end
        // (<https://www.w3.org/TR/css-align-3/#baseline-align-self>)
        stylo::AlignFlags::BASELINE => taffy::AlignContent::START,
        stylo::AlignFlags::LAST_BASELINE => taffy::AlignContent::END,
        // Should never be hit. But no real reason to panic here.
        _ => return taffy::AlignContent::NORMAL,
    };
    let is_block_container = matches!(
        display.inside(),
        stylo::DisplayInside::Flow
            | stylo::DisplayInside::FlowRoot
            | stylo::DisplayInside::TableCell
    );
    let safe = primary.flags().contains(stylo::AlignFlags::SAFE)
        || (is_block_container && !primary.flags().contains(stylo::AlignFlags::UNSAFE));
    if safe {
        align.safety = taffy::AlignmentSafety::Safe;
    }
    align
}

/// The `align-content` value that a table cell's `vertical-align` is equivalent to when the
/// cell's own `align-content` is `normal`: `top`, `middle` and `bottom` behave as
/// `safe start`, `safe center` and `safe end` respectively
/// (<https://drafts.csswg.org/css-align-3/#distribution-block>). Baseline alignment is left
/// to the table's row layout.
#[cfg(feature = "block")]
#[inline]
pub fn table_cell_vertical_align(style: &stylo::ComputedValues) -> Option<taffy::AlignContent> {
    let box_styles = style.get_box();
    let mut align = match (
        box_styles.clone_alignment_baseline(),
        box_styles.clone_baseline_shift(),
    ) {
        (_, stylo::BaselineShift::Keyword(stylo::BaselineShiftKeyword::Top)) => {
            taffy::AlignContent::START
        }
        (_, stylo::BaselineShift::Keyword(stylo::BaselineShiftKeyword::Bottom)) => {
            taffy::AlignContent::END
        }
        (stylo::AlignmentBaseline::Middle, _) => taffy::AlignContent::CENTER,
        _ => return None,
    };
    align.safety = taffy::AlignmentSafety::Safe;
    Some(align)
}

/// Convert `justify-content`, resolving the physical `left`/`right` keywords against the
/// container's flex main axis and text direction. `left`/`right` behave as `start` when the
/// main axis is not the inline axis (<https://www.w3.org/TR/css-align-3/#positional-values>).
#[inline]
pub fn justify_content(
    input: stylo::ContentDistribution,
    flex_direction: stylo::FlexDirection,
    direction: stylo::Direction,
    display: stylo::Display,
) -> taffy::AlignContent {
    let is_row = matches!(
        flex_direction,
        stylo::FlexDirection::Row | stylo::FlexDirection::RowReverse
    );
    let is_rtl = matches!(direction, stylo::Direction::Rtl);
    justify_content_in(input, is_row.then_some(is_rtl), display)
}

/// Convert `justify-content`. `left_is_end` is how the physical `left` keyword resolves when the
/// main axis is parallel to the left/right axis; `None` makes `left`/`right` behave as `start`.
#[inline]
pub fn justify_content_in(
    input: stylo::ContentDistribution,
    left_is_end: Option<bool>,
    display: stylo::Display,
) -> taffy::AlignContent {
    let primary = input.primary();
    let is_right = match primary.value() {
        stylo::AlignFlags::LEFT => false,
        stylo::AlignFlags::RIGHT => true,
        _ => return self::content_alignment(input, display),
    };
    let mut align = match left_is_end {
        Some(left_is_end) if is_right != left_is_end => taffy::AlignContent::END,
        _ => taffy::AlignContent::START,
    };
    if primary.flags().contains(stylo::AlignFlags::SAFE) {
        align.safety = taffy::AlignmentSafety::Safe;
    }
    align
}

/// Convert item alignment values (`align-items`/`align-self`/`justify-items`/`justify-self`),
/// resolving the physical `left`/`right` keywords against `is_horiz_rtl`: whether the axis being
/// aligned is a horizontal axis with right-to-left text direction. Pass `false` for the vertical
/// axis (<https://www.w3.org/TR/css-align-3/#positional-values>).
///
/// `auto` maps to `None` (for the `*-self` properties Taffy then defers to the container's
/// `*-items` value). `normal` maps to Taffy's `NORMAL` keyword, which Taffy resolves according
/// to the layout mode of the box being aligned.
#[inline]
pub fn item_alignment(input: stylo::AlignFlags, is_horiz_rtl: bool) -> Option<taffy::AlignItems> {
    let mut align = match input.value() {
        stylo::AlignFlags::AUTO => return None,
        stylo::AlignFlags::NORMAL => return Some(taffy::AlignItems::NORMAL),
        stylo::AlignFlags::STRETCH => taffy::AlignItems::STRETCH,
        stylo::AlignFlags::FLEX_START => taffy::AlignItems::FLEX_START,
        stylo::AlignFlags::FLEX_END => taffy::AlignItems::FLEX_END,
        stylo::AlignFlags::SELF_START => taffy::AlignItems::SELF_START,
        stylo::AlignFlags::SELF_END => taffy::AlignItems::SELF_END,
        stylo::AlignFlags::START => taffy::AlignItems::START,
        stylo::AlignFlags::END => taffy::AlignItems::END,
        stylo::AlignFlags::LEFT if is_horiz_rtl => taffy::AlignItems::END,
        stylo::AlignFlags::LEFT => taffy::AlignItems::START,
        stylo::AlignFlags::RIGHT if is_horiz_rtl => taffy::AlignItems::START,
        stylo::AlignFlags::RIGHT => taffy::AlignItems::END,
        stylo::AlignFlags::CENTER => taffy::AlignItems::CENTER,
        stylo::AlignFlags::BASELINE => taffy::AlignItems::BASELINE,
        // Taffy does not support last-baseline alignment, so map it to its
        // fallback alignment of `self-end` (https://www.w3.org/TR/css-align-3/#baseline-values)
        stylo::AlignFlags::LAST_BASELINE => taffy::AlignItems::END,
        // Should never be hit. But no real reason to panic here.
        _ => return None,
    };
    if input.flags().contains(stylo::AlignFlags::SAFE) {
        align.safety = taffy::AlignmentSafety::Safe;
    } else if input.flags().contains(stylo::AlignFlags::UNSAFE) {
        align.safety = taffy::AlignmentSafety::Unsafe;
    }
    Some(align)
}

/// Convert the `align-self`/`justify-self` value of an absolutely positioned box. This only
/// differs from [`item_alignment`] in its handling of the physical `left`/`right` keywords.
///
/// `is_inline_axis` is whether the property aligns the box in its inline (horizontal) axis.
/// The physical `left`/`right` keywords behave as `start` in the block axis.
///
/// `is_item_rtl` is whether the box's own `direction` is `rtl`. Taffy resolves alignment
/// relative to the *containing block's* direction, which is not known here, so the physical
/// `left`/`right` keywords are expressed in terms of the box's own direction as
/// `self-start`/`self-end` (which Taffy resolves against the containing block's direction).
#[inline]
pub fn oof_item_alignment(
    input: stylo::AlignFlags,
    is_inline_axis: bool,
    is_item_rtl: bool,
) -> Option<taffy::AlignItems> {
    let mut align = match input.value() {
        stylo::AlignFlags::LEFT | stylo::AlignFlags::RIGHT if !is_inline_axis => {
            taffy::AlignItems::START
        }
        stylo::AlignFlags::LEFT if is_item_rtl => taffy::AlignItems::SELF_END,
        stylo::AlignFlags::LEFT => taffy::AlignItems::SELF_START,
        stylo::AlignFlags::RIGHT if is_item_rtl => taffy::AlignItems::SELF_START,
        stylo::AlignFlags::RIGHT => taffy::AlignItems::SELF_END,
        _ => return item_alignment(input, is_item_rtl),
    };
    if input.flags().contains(stylo::AlignFlags::SAFE) {
        align.safety = taffy::AlignmentSafety::Safe;
    } else if input.flags().contains(stylo::AlignFlags::UNSAFE) {
        align.safety = taffy::AlignmentSafety::Unsafe;
    }
    Some(align)
}

/// Convert a container's `align-items`/`justify-items`. `auto` is not a valid value of these
/// properties, so it is treated as `normal`.
#[inline]
pub fn default_item_alignment(input: stylo::AlignFlags, is_horiz_rtl: bool) -> taffy::AlignItems {
    item_alignment(input, is_horiz_rtl).unwrap_or(taffy::AlignItems::NORMAL)
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
    match input {
        stylo::FlexBasis::Content => taffy::Dimension::content(),
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
    if input.contains(stylo::FlexWrap::BALANCE) {
        if input.contains(stylo::FlexWrap::WRAP_REVERSE) {
            taffy::FlexWrap::BalanceReverse
        } else {
            taffy::FlexWrap::Balance
        }
    } else if input.contains(stylo::FlexWrap::WRAP_REVERSE) {
        taffy::FlexWrap::WrapReverse
    } else if input.contains(stylo::FlexWrap::WRAP) {
        taffy::FlexWrap::Wrap
    } else {
        taffy::FlexWrap::NoWrap
    }
}

#[inline]
#[cfg(feature = "flexbox")]
pub fn flex_line_count(input: i32) -> u16 {
    saturating_u16(input).max(1)
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

#[inline]
#[cfg(feature = "grid")]
pub fn grid_line(input: &stylo::GridLine) -> taffy::GridPlacement<Atom> {
    if input.is_auto() {
        taffy::GridPlacement::Auto
    } else if input.is_span {
        if input.ident.0 != stylo::atom!("") {
            taffy::GridPlacement::NamedSpan(input.ident.0.clone(), saturating_u16(input.line_num))
        } else {
            taffy::GridPlacement::Span(saturating_u16(input.line_num))
        }
    } else if input.ident.0 != stylo::atom!("") {
        taffy::GridPlacement::NamedLine(input.ident.0.clone(), saturating_i16(input.line_num))
    } else if input.line_num != 0 {
        taffy::style_helpers::line(saturating_i16(input.line_num))
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
        row_start: saturating_u16(input.rows.start),
        row_end: saturating_u16(input.rows.end),
        column_start: saturating_u16(input.columns.start),
        column_end: saturating_u16(input.columns.end),
    }
}

#[inline]
#[cfg(feature = "grid")]
fn grid_template_areas(input: &stylo::GridTemplateAreas) -> Option<taffy::GridTemplateAreas<Atom>> {
    match input {
        stylo::GridTemplateAreas::None => None,
        stylo::GridTemplateAreas::Areas(template_areas_arc) => {
            let template = &template_areas_arc.0;
            Some(taffy::GridTemplateAreas {
                areas: crate::wrapper::GridAreaWrapper(&template.areas)
                    .into_iter()
                    .collect(),
                row_count: saturating_u16(template.strings.len()),
                column_count: saturating_u16(template.width),
            })
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
        stylo::RepeatCount::Number(val) => taffy::RepetitionCount::Count(saturating_u16(val)),
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

                // Are these valid? Taffy doesn't support this in any case
                stylo::TrackBreadth::Flex(_) => unreachable!(),
                stylo::TrackBreadth::Auto => unreachable!(),
                stylo::TrackBreadth::MinContent => unreachable!(),
                stylo::TrackBreadth::MaxContent => unreachable!(),
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
        stylo::TrackBreadth::Flex(_) => taffy::MinTrackSizingFunction::AUTO,
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
        stylo::TrackBreadth::Flex(val) => taffy::MaxTrackSizingFunction::from_fr(val.0),
        stylo::TrackBreadth::Auto => taffy::MaxTrackSizingFunction::AUTO,
        stylo::TrackBreadth::MinContent => taffy::MaxTrackSizingFunction::MIN_CONTENT,
        stylo::TrackBreadth::MaxContent => taffy::MaxTrackSizingFunction::MAX_CONTENT,
    }
}

/// Eagerly convert an entire [`stylo::ComputedValues`] into a [`taffy::Style`]
/// Convert a stylo style into a concrete [`taffy::Style`] expressed in the axes of `layout_wm`, the
/// writing mode of the algorithm laying the box out (see [`crate::writing_mode`]).
pub fn to_taffy_style_in(
    style: &stylo::ComputedValues,
    layout_wm: stylo::WritingMode,
) -> taffy::Style<Atom> {
    use crate::writing_mode::WritingModeExt;
    let mut taffy_style = to_taffy_style(style);
    layout_wm.transpose_style(&mut taffy_style);
    taffy_style
}

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
        item_is_compressible_replaced: false,
        position: self::position(style.clone_position()),
        overflow: taffy::Point {
            x: self::overflow(style.clone_overflow_x()),
            y: self::overflow(style.clone_overflow_y()),
        },
        direction: self::direction(style.clone_direction()),
        scrollbar_width: 0.0,
        contain: self::contain(style.clone_contain(), style.clone_display()),

        #[cfg(feature = "floats")]
        float: self::float(style.clone_float()),
        #[cfg(feature = "floats")]
        clear: self::clear(style.clone_clear()),

        size: taffy::Size {
            width: self::dimension(&pos.width),
            height: self::dimension(&pos.height),
        },
        min_size: taffy::Size {
            width: self::min_size(&pos.min_width),
            height: self::min_size(&pos.min_height),
        },
        max_size: taffy::Size {
            width: self::max_size(&pos.max_width),
            height: self::max_size(&pos.max_height),
        },
        aspect_ratio: self::aspect_ratio(pos.aspect_ratio),

        inset: self::inset_rect(style),
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
        #[cfg(any(feature = "flexbox", feature = "block", feature = "grid"))]
        align_content: self::content_alignment(pos.align_content, display),
        #[cfg(any(feature = "flexbox", feature = "grid"))]
        justify_content: self::justify_content(
            pos.justify_content,
            pos.flex_direction,
            style.clone_direction(),
            display,
        ),
        #[cfg(any(feature = "flexbox", feature = "grid"))]
        align_items: self::default_item_alignment(pos.align_items.0, false),
        #[cfg(any(feature = "flexbox", feature = "grid"))]
        align_self: self::item_alignment(pos.align_self.0, false),
        #[cfg(feature = "grid")]
        justify_items: self::default_item_alignment(
            (pos.justify_items.computed.0).0,
            style.clone_direction() == stylo::Direction::Rtl,
        ),
        #[cfg(feature = "grid")]
        justify_self: self::item_alignment(
            pos.justify_self.0,
            style.clone_direction() == stylo::Direction::Rtl,
        ),
        #[cfg(feature = "block")]
        text_align: self::text_align(style.clone_text_align()),

        // Flexbox
        #[cfg(feature = "flexbox")]
        flex_direction: self::flex_direction(pos.flex_direction),
        #[cfg(feature = "flexbox")]
        flex_wrap: self::flex_wrap(pos.flex_wrap),
        #[cfg(feature = "flexbox")]
        flex_line_count: self::flex_line_count(pos.flex_line_count),
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
