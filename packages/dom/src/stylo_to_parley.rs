//! Conversion functions from Stylo types to Parley types
use crate::node::TextBrush;
use crate::util::ToPenikoColor;

// Module of type aliases so we can refer to stylo types with nicer names
pub(crate) mod stylo {
    pub(crate) use style::computed_values::white_space_collapse::T as WhiteSpaceCollapse;
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

// TextStyle struct that owns all the data that need a lifetime
// Should not be constructed externally since it is a self-referencing struct and extra care has to be taken to keep it safe
#[derive(Default)]
pub(crate) struct OwnedTextStyle {
    #[allow(unused)]
    fonts: Vec<String>,
    #[allow(unused)]
    font_stack: Box<[parley::FontFamily<'static>]>,
    text_style: parley::TextStyle<'static, TextBrush>,
}

// Not safe as of right now and therefore commented out. Needs to to have a non-static lifetime that is bound to the `self` argument
/*
impl Deref for OwnedTextStyle {
    type Target = parley::TextStyle<'static, TextBrush>;

    fn deref(&self) -> &Self::Target {
        &self.text_style
    }
}
*/

impl OwnedTextStyle {
    #[allow(clippy::needless_lifetimes)]
    pub(crate) fn take_style<'a>(&'a mut self) -> parley::TextStyle<'a, TextBrush> {
        std::mem::take(&mut self.text_style)
    }
}

pub(crate) fn style(style: &stylo::ComputedValues) -> OwnedTextStyle {
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

    // Convert Bold/Italic
    let font_weight = parley::FontWeight::new(font_styles.font_weight.value());
    let font_style = match font_styles.font_style {
        stylo::FontStyle::NORMAL => parley::FontStyle::Normal,
        stylo::FontStyle::ITALIC => parley::FontStyle::Italic,
        val => parley::FontStyle::Oblique(Some(val.oblique_degrees())),
    };

    // Convert font family
    let mut fonts = Vec::new();
    let families: Box<[_]> = font_styles
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

                    let name = name.to_string();
                    fonts.push(name);
                    let name = fonts.last().unwrap() as &str;
                    let name = name as *const str;
                    // this is safe since the string won't be reallocated
                    // and gets deallocated when the self-referencing struct
                    // it's being part of gets dropped
                    let name = unsafe { &*name };
                    break 'ret parley::FontFamily::Named(name);
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

    let families_ptr = &*families as *const [parley::FontFamily<'static>];
    // this is safe since the array won't be reallocated
    // and gets deallocated when the self-referencing struct
    // it's being part of gets dropped
    let families_ref = unsafe { &*families_ptr };

    // Convert text colour
    let color = itext_styles.color.as_peniko();

    let decoration_brush = style
        .get_text()
        .text_decoration_color
        .as_absolute()
        .map(ToPenikoColor::as_peniko)
        .map(|color| TextBrush::Normal(peniko::Brush::Solid(color)));

    OwnedTextStyle {
        fonts,
        font_stack: families,
        text_style: parley::TextStyle {
            font_stack: parley::FontStack::List(families_ref),
            font_size,
            font_stretch: Default::default(),
            font_style,
            font_weight,
            font_variations: parley::FontSettings::List(&[]),
            font_features: parley::FontSettings::List(&[]),
            locale: Default::default(),
            brush: TextBrush::Normal(peniko::Brush::Solid(color)),
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
            letter_spacing: itext_styles.letter_spacing.0.px(),
        },
    }
}
