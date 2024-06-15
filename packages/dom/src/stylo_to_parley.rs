//! Conversion functions from Stylo types to Parley types
use crate::node::TextBrush;

// Module of type aliases so we can refer to stylo types with nicer names
pub(crate) mod stylo {
    pub(crate) use style::computed_values::white_space::T as WhiteSpaceCollapse;
    pub(crate) use style::properties::ComputedValues;
    pub(crate) use style::values::computed::font::FontStyle;
    pub(crate) use style::values::computed::font::GenericFontFamily;
    pub(crate) use style::values::computed::font::SingleFontFamily;
    pub(crate) use style::values::generics::text::LineHeight;
}

pub(crate) mod parley {
    pub(crate) use parley::style::*;
}

pub(crate) fn white_space(input: stylo::WhiteSpaceCollapse) -> parley::WhiteSpaceCollapse {
    match input {
        stylo::WhiteSpaceCollapse::Normal => parley::WhiteSpaceCollapse::Collapse,
        stylo::WhiteSpaceCollapse::Pre => parley::WhiteSpaceCollapse::Preserve,
        stylo::WhiteSpaceCollapse::Nowrap => parley::WhiteSpaceCollapse::Collapse,
        stylo::WhiteSpaceCollapse::PreWrap => parley::WhiteSpaceCollapse::Preserve,
        stylo::WhiteSpaceCollapse::PreLine => parley::WhiteSpaceCollapse::Preserve,
    }
}

pub(crate) fn style(style: &stylo::ComputedValues) -> parley::TextStyle<'static, TextBrush> {
    let font_styles = style.get_font();
    // let text_styles = style.get_text();
    let itext_styles = style.get_inherited_text();

    // Convert font size and line height
    let font_size = font_styles.font_size.used_size.0.px();
    let line_height: f32 = match itext_styles.line_height {
        stylo::LineHeight::Normal => font_size * 1.2,
        stylo::LineHeight::Number(num) => font_size * num.0,
        stylo::LineHeight::Length(value) => value.0.px(),
    };
    // Parley expects line height as a multiple of font size!
    let line_height = line_height / font_size;

    // Convert Bold/Italic
    let font_weight = parley::FontWeight::new(font_styles.font_weight.value());
    let font_style = match font_styles.font_style {
        stylo::FontStyle::NORMAL => parley::FontStyle::Normal,
        stylo::FontStyle::ITALIC => parley::FontStyle::Italic,
        val => parley::FontStyle::Oblique(Some(val.oblique_degrees())),
    };

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

                    // TODO: fix leak!
                    parley::FontFamily::Named(name.to_string().leak())
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

    // TODO: fix leak!
    let families = Box::leak(families.into_boxed_slice());

    // Convert text colour
    let [r, g, b, a] = itext_styles
        .color
        .to_color_space(style::color::ColorSpace::Srgb)
        .raw_components()
        .map(|f| (f * 255.0) as u8);
    let color = peniko::Color { r, g, b, a };

    parley::TextStyle {
        // font_stack: parley::FontStack::Single(FontFamily::Generic(GenericFamily::SystemUi)),
        font_stack: parley::FontStack::List(families),
        font_size,
        font_stretch: Default::default(),
        font_style,
        font_weight,
        font_variations: parley::FontSettings::List(&[]),
        font_features: parley::FontSettings::List(&[]),
        locale: Default::default(),
        brush: TextBrush { color },
        has_underline: itext_styles.text_decorations_in_effect.underline,
        underline_offset: Default::default(),
        underline_size: Default::default(),
        underline_brush: Default::default(),
        has_strikethrough: itext_styles.text_decorations_in_effect.line_through,
        strikethrough_offset: Default::default(),
        strikethrough_size: Default::default(),
        strikethrough_brush: Default::default(),
        line_height,
        word_spacing: Default::default(),
        letter_spacing: itext_styles.letter_spacing.0.px(),
    }
}
