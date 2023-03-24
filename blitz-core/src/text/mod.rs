use vello::{
    fello::{raw::FontRef, MetadataProvider},
    glyph::GlyphContext,
    kurbo::Affine,
    peniko::{Brush, Font},
    SceneBuilder,
};

const FONT_DATA: &[u8] = include_bytes!("Roboto-Regular.ttf");

pub struct TextContext {
    gcx: GlyphContext,
}

impl Default for TextContext {
    fn default() -> Self {
        Self {
            gcx: GlyphContext::new(),
        }
    }
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
