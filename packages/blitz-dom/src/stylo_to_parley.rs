//! Conversion functions from Stylo types to Parley types
use std::borrow::Cow;

use style::values::computed::Length;

use crate::node::TextBrush;
use crate::util::ToColorColor;

// Module of type aliases so we can refer to stylo types with nicer names
pub(crate) mod stylo {
    pub(crate) use style::computed_values::white_space_collapse::T as WhiteSpaceCollapse;
    pub(crate) use style::computed_values::overflow_wrap::T as OverflowWrap;
    pub(crate) use style::computed_values::word_break::T as WordBreak;
    pub(crate) use style::properties::ComputedValues;
    pub(crate) use style::values::computed::font::FontStyle;
    pub(crate) use style::values::computed::font::GenericFontFamily;
    pub(crate) use style::values::computed::font::LineHeight;
    pub(crate) use style::values::computed::font::SingleFontFamily;
}

pub(crate) mod parley {
    pub(crate) use parley::style::*;
}

pub(crate) fn white_space_collapse(input: stylo::WhiteSpaceCollapse) -> parley::WhiteSpaceCollapse {
    match input {
        stylo::WhiteSpaceCollapse::Collapse => parley::WhiteSpaceCollapse::Collapse,
        stylo::WhiteSpaceCollapse::Preserve => parley::WhiteSpaceCollapse::Preserve,

        // TODO: Implement PreserveBreaks and BreakSpaces modes
        stylo::WhiteSpaceCollapse::PreserveBreaks => parley::WhiteSpaceCollapse::Preserve,
        stylo::WhiteSpaceCollapse::BreakSpaces => parley::WhiteSpaceCollapse::Preserve,
    }
}

pub(crate) fn overflow_wrap(input: stylo::OverflowWrap) -> parley::OverflowWrap {
    match input {
        stylo::OverflowWrap::Normal => parley::OverflowWrap::Normal,
        stylo::OverflowWrap::Anywhere => parley::OverflowWrap::Anywhere,
        stylo::OverflowWrap::BreakWord => parley::OverflowWrap::BreakWord,
    }
}

pub(crate) fn word_break(input: stylo::WordBreak) -> parley::WordBreakStrength {
    match input {
        stylo::WordBreak::Normal => parley::WordBreakStrength::Normal,
        stylo::WordBreak::BreakAll => parley::WordBreakStrength::BreakAll,
        stylo::WordBreak::KeepAll => parley::WordBreakStrength::KeepAll,
    }
}

pub(crate) fn style(
    span_id: usize,
    style: &stylo::ComputedValues,
) -> parley::TextStyle<'static, TextBrush> {
    let font_styles = style.get_font();
    // let text_styles = style.get_text();
    let itext_styles = style.get_inherited_text();

    // Convert font size and line height
    let font_size = font_styles.font_size.used_size.0.px();
    let line_height: f32 = match font_styles.line_height {
        stylo::LineHeight::Normal => font_size * 1.2,
        stylo::LineHeight::Number(num) => font_size * num.0,
        stylo::LineHeight::Length(value) => value.0.px(),
    };
    // Parley expects line height as a multiple of font size!
    let line_height = line_height / font_size;

    let letter_spacing = itext_styles
        .letter_spacing
        .0
        .resolve(Length::new(font_size))
        .px();

    // Convert Bold/Italic
    let font_weight = parley::FontWeight::new(font_styles.font_weight.value());
    let font_style = match font_styles.font_style {
        stylo::FontStyle::NORMAL => parley::FontStyle::Normal,
        stylo::FontStyle::ITALIC => parley::FontStyle::Italic,
        val => parley::FontStyle::Oblique(Some(val.oblique_degrees())),
    };
    let font_width = parley::FontWidth::from_percentage(font_styles.font_stretch.0.to_float());
    let font_variations: Vec<_> = font_styles
        .font_variation_settings
        .0
        .iter()
        .map(|v| parley::FontVariation {
            tag: v.tag.0,
            value: v.value,
        })
        .collect();

    // Convert font family
    let families: Vec<_> = font_styles
        .font_family
        .families
        .list
        .iter()
        .map(|family| match family {
            stylo::SingleFontFamily::FamilyName(name) => {
                'ret: {
                    let name = name.name.as_ref();

                    // Legacy web compatibility
                    #[cfg(target_vendor = "apple")]
                    if name == "-apple-system" {
                        break 'ret parley::FontFamily::Generic(parley::GenericFamily::SystemUi);
                    }
                    #[cfg(target_os = "macos")]
                    if name == "BlinkMacSystemFont" {
                        break 'ret parley::FontFamily::Generic(parley::GenericFamily::SystemUi);
                    }

                    break 'ret parley::FontFamily::Named(Cow::Owned(name.to_string()));
                }
            }
            stylo::SingleFontFamily::Generic(generic) => {
                parley::FontFamily::Generic(match generic {
                    stylo::GenericFontFamily::None => parley::GenericFamily::SansSerif,
                    stylo::GenericFontFamily::Serif => parley::GenericFamily::Serif,
                    stylo::GenericFontFamily::SansSerif => parley::GenericFamily::SansSerif,
                    stylo::GenericFontFamily::Monospace => parley::GenericFamily::Monospace,
                    stylo::GenericFontFamily::Cursive => parley::GenericFamily::Cursive,
                    stylo::GenericFontFamily::Fantasy => parley::GenericFamily::Fantasy,
                    stylo::GenericFontFamily::SystemUi => parley::GenericFamily::SystemUi,
                })
            }
        })
        .collect();

    // Convert text colour
    let color = itext_styles.color.as_color_color();

    let decoration_brush = style
        .get_text()
        .text_decoration_color
        .as_absolute()
        .map(ToColorColor::as_color_color)
        .map(TextBrush::from_color);

    parley::TextStyle {
        // font_stack: parley::FontStack::Single(FontFamily::Generic(GenericFamily::SystemUi)),
        font_stack: parley::FontStack::List(Cow::Owned(families)),
        font_size,
        font_width,
        font_style,
        font_weight,
        font_variations: parley::FontSettings::List(Cow::Owned(font_variations)),
        font_features: parley::FontSettings::List(Cow::Borrowed(&[])),
        locale: Default::default(),
        brush: TextBrush::from_id_and_color(span_id, color),
        has_underline: itext_styles.text_decorations_in_effect.underline,
        underline_offset: Default::default(),
        underline_size: Default::default(),
        underline_brush: decoration_brush.clone(),
        has_strikethrough: itext_styles.text_decorations_in_effect.line_through,
        strikethrough_offset: Default::default(),
        strikethrough_size: Default::default(),
        strikethrough_brush: decoration_brush,
        line_height,
        word_spacing: Default::default(),
        letter_spacing,
        overflow_wrap: overflow_wrap(itext_styles.overflow_wrap),
        word_break: word_break(itext_styles.word_break),
    }
}
