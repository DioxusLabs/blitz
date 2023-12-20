// use dioxus_native_core::node::OwnedAttributeValue;
// use dioxus_native_core::prelude::*;
// use dioxus_native_core_macro::partial_derive_state;
// use lightningcss::properties::font::AbsoluteFontSize;
// use lightningcss::properties::font::FontSize as FontSizeProperty;
// use lightningcss::properties::font::RelativeFontSize;
// use lightningcss::traits::Parse;
// use lightningcss::values::length::LengthValue;
// use lightningcss::values::percentage::DimensionPercentage;
use shipyard::Component;
use vello::{
    fello::{raw::FontRef, MetadataProvider},
    glyph::GlyphContext,
    kurbo::Affine,
    peniko::{Brush, Font},
    SceneBuilder,
};

const FONT_DATA: &[u8] = include_bytes!("Roboto-Regular.ttf");

#[derive(Default)]
pub struct TextContext {
    gcx: GlyphContext,
}

impl TextContext {
    pub fn add(
        &mut self,
        builder: &mut SceneBuilder,
        font: Option<&Font>,
        size: f32,
        brush: Option<impl Into<Brush>>,
        transform: Affine,
        text: &str,
    ) {
        let font = font.and_then(to_font_ref).unwrap_or_else(default_font);
        let fello_size = vello::fello::Size::new(size);
        let charmap = font.charmap();
        let metrics = font.metrics(fello_size, Default::default());
        let line_height = metrics.ascent - metrics.descent + metrics.leading;
        let glyph_metrics = font.glyph_metrics(fello_size, Default::default());
        let mut pen_x = 0f64;
        let mut pen_y = 0f64;
        let vars: [(&str, f32); 0] = [];
        let mut provider = self.gcx.new_provider(&font, None, size, false, vars);
        let brush = brush.map(Into::into);
        for ch in text.chars() {
            if ch == '\n' {
                pen_y += line_height as f64;
                pen_x = 0.0;
                continue;
            }
            let gid = charmap.map(ch).unwrap_or_default();
            let advance = glyph_metrics.advance_width(gid).unwrap_or_default() as f64;
            if let Some(glyph) = provider.get(gid.to_u16(), brush.as_ref()) {
                let xform = transform
                    * Affine::translate((pen_x, pen_y))
                    * Affine::scale_non_uniform(1.0, -1.0);
                builder.append(&glyph, Some(xform));
            }
            pen_x += advance;
        }
    }

    pub fn get_text_size<'a>(
        &'a mut self,
        font: Option<&'a Font>,
        size: f32,
        text: &str,
    ) -> (f64, f64) {
        let font = font.and_then(to_font_ref).unwrap_or_else(default_font);
        let fello_size = vello::fello::Size::new(size);
        let charmap = font.charmap();
        let metrics = font.metrics(fello_size, Default::default());
        let line_height = metrics.ascent - metrics.descent + metrics.leading;
        let glyph_metrics = font.glyph_metrics(fello_size, Default::default());
        let mut width = 0f64;
        let mut height = line_height as f64;
        for ch in text.chars() {
            if ch == '\n' {
                height += line_height as f64;
                continue;
            }
            let gid = charmap.map(ch).unwrap_or_default();
            let advance = glyph_metrics.advance_width(gid).unwrap_or_default() as f64;
            width += advance;
        }
        (width, height)
    }
}

fn to_font_ref(font: &Font) -> Option<FontRef> {
    use vello::fello::raw::FileRef;
    let file_ref = FileRef::new(font.data.as_ref()).ok()?;
    match file_ref {
        FileRef::Font(font) => Some(font),
        FileRef::Collection(collection) => collection.get(font.index).ok(),
    }
}

fn default_font<'a>() -> FontRef<'a> {
    FontRef::new(FONT_DATA).unwrap()
}

#[derive(Clone, PartialEq, Debug, Component)]
pub(crate) struct FontSize(pub f32);
pub const DEFAULT_FONT_SIZE: f32 = 16.0;

impl Default for FontSize {
    fn default() -> Self {
        FontSize(DEFAULT_FONT_SIZE)
    }
}

// fn parse_font_size_from_attr(
//     css_value: &OwnedAttributeValue,
//     parent_font_size: f32,
//     root_font_size: f32,
// ) -> Option<f32> {
//     match css_value {
//         OwnedAttributeValue::Text(n) => {
//             // css font-size parse.
//             // not support
//             // 1. calc,
//             // 3. relative font size. (smaller, larger)
//             match FontSizeProperty::parse_string(n) {
//                 Ok(FontSizeProperty::Length(length)) => match length {
//                     DimensionPercentage::Dimension(l) => match l {
//                         LengthValue::Rem(v) => Some(v * root_font_size),
//                         LengthValue::Em(v) => Some(v * parent_font_size),
//                         _ => l.to_px(),
//                     },
//                     // same with em.
//                     DimensionPercentage::Percentage(p) => Some(p.0 * parent_font_size),
//                     DimensionPercentage::Calc(_c) => None,
//                 },
//                 Ok(FontSizeProperty::Absolute(abs_val)) => {
//                     let factor = match abs_val {
//                         AbsoluteFontSize::XXSmall => 0.6,
//                         AbsoluteFontSize::XSmall => 0.75,
//                         AbsoluteFontSize::Small => 0.89, // 8/9
//                         AbsoluteFontSize::Medium => 1.0,
//                         AbsoluteFontSize::Large => 1.25,
//                         AbsoluteFontSize::XLarge => 1.5,
//                         AbsoluteFontSize::XXLarge => 2.0,
//                         AbsoluteFontSize::XXXLarge => 3.0,
//                     };
//                     Some(factor * root_font_size)
//                 }
//                 Ok(FontSizeProperty::Relative(rel_val)) => {
//                     let factor = match rel_val {
//                         RelativeFontSize::Smaller => 0.8,
//                         RelativeFontSize::Larger => 1.25,
//                     };
//                     Some(factor * parent_font_size)
//                 }
//                 _ => None,
//             }
//         }
//         OwnedAttributeValue::Float(n) => Some(n.to_owned() as f32),
//         OwnedAttributeValue::Int(n) => Some(n.to_owned() as f32),
//         _ => None,
//     }
// }
