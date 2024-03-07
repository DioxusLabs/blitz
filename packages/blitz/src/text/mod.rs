use std::cell::RefCell;

use glyphon::{
    Attrs, Buffer, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer,
};
use vello::{glyph::skrifa::raw::FileRef, skrifa::prelude::*, Scene};
use vello::{
    glyph::GlyphContext,
    kurbo::Affine,
    peniko::{Brush, Font},
};

const FONT_DATA: &[u8] = include_bytes!("Roboto-Regular.ttf");

#[derive(Default)]
pub struct TextContext {
    gcx: RefCell<GlyphContext>,
}

impl TextContext {
    /// way more to this than meets the eye
    /// We'll want to add a parameter for style stacks (underline, fontweight, strike-thru, etc)
    /// https://github.com/dfrg/parley/blob/master/src/resolve/mod.rs
    pub fn add(
        &self,
        builder: &mut Scene,
        font: Option<&Font>,
        size: f32,
        brush: Option<impl Into<Brush>>,
        transform: Affine,
        text: &str,
    ) {
        let font = font.and_then(to_font_ref).unwrap_or_else(default_font);
        let fello_size = Size::new(size);
        let charmap = font.charmap();
        let metrics = font.metrics(fello_size, LocationRef::default());
        let line_height = metrics.ascent - metrics.descent + metrics.leading;
        let glyph_metrics = font.glyph_metrics(fello_size, LocationRef::default());
        let mut pen_x = 0f64;
        let mut pen_y = 0f64;
        let vars: [(&str, f32); 0] = [];

        let mut gcx = self.gcx.borrow_mut();

        let mut provider = gcx.new_provider(&font, size, false, vars);
        // let mut provider = gcx.new_provider(&font, None, size, false, vars);
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
        &'a self,
        font: Option<&'a Font>,
        size: f32,
        text: &str,
    ) -> (f64, f64) {
        let font = font.and_then(to_font_ref).unwrap_or_else(default_font);
        let fello_size = Size::new(size);
        let charmap = font.charmap();
        let metrics = font.metrics(fello_size, LocationRef::default());
        let line_height = metrics.ascent - metrics.descent + metrics.leading;
        let glyph_metrics = font.glyph_metrics(fello_size, LocationRef::default());
        let mut max_width = 0;
        let mut cur_width = 0f64;
        let mut height = line_height as f64;

        for ch in text.chars() {
            if ch == '\n' {
                height += line_height as f64;
                max_width = max_width.max(cur_width as i32);
                cur_width = 0.0;
                continue;
            }
            let gid = charmap.map(ch).unwrap_or_default();
            let advance = glyph_metrics.advance_width(gid).unwrap_or_default() as f64;
            cur_width += advance;
        }

        max_width = max_width.max(cur_width as i32);

        (max_width as _, height)
    }
}

fn to_font_ref(font: &Font) -> Option<FontRef> {
    let file_ref = FileRef::new(font.data.as_ref()).ok()?;
    match file_ref {
        FileRef::Font(font) => Some(font),
        FileRef::Collection(collection) => collection.get(font.index).ok(),
    }
}

fn default_font<'a>() -> FontRef<'a> {
    FontRef::new(FONT_DATA).unwrap()
}
