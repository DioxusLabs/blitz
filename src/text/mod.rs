use vello::{
    glyph::{
        pinot::{self, FontRef, TableProvider},
        GlyphContext,
    },
    kurbo::Affine,
    peniko::Brush,
    SceneBuilder,
};

// This is very much a hack to get things working.
// On Windows, can set this to "c:\\Windows\\Fonts\\seguiemj.ttf" to get color emoji
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
        font: Option<&FontRef>,
        size: f32,
        brush: Option<impl Into<Brush>>,
        transform: Affine,
        text: &str,
    ) {
        let brush = brush.map(|brush| brush.into());
        let brush = brush.as_ref();
        let font = font.unwrap_or(&FontRef {
            data: FONT_DATA,
            offset: 0,
        });
        if let Some(cmap) = font.cmap() {
            if let Some(hmtx) = font.hmtx() {
                let upem = font.head().map(|head| head.units_per_em()).unwrap_or(1000) as f64;
                let scale = size as f64 / upem;
                let vars: [(pinot::types::Tag, f32); 0] = [];
                let mut provider = self.gcx.new_provider(font, None, size, false, vars);
                let hmetrics = hmtx.hmetrics();
                let default_advance = hmetrics
                    .get(hmetrics.len().saturating_sub(1))
                    .map(|h| h.advance_width)
                    .unwrap_or(0);
                let mut pen_x = 0f64;
                for ch in text.chars() {
                    let gid = cmap.map(ch as u32).unwrap_or(0);
                    let advance = hmetrics
                        .get(gid as usize)
                        .map(|h| h.advance_width)
                        .unwrap_or(default_advance) as f64
                        * scale;
                    if let Some(glyph) = provider.get(gid, brush) {
                        let xform = transform
                            * Affine::translate((pen_x, 0.0))
                            * Affine::scale_non_uniform(1.0, -1.0);
                        builder.append(&glyph, Some(xform));
                    }
                    pen_x += advance;
                }
            }
        }
    }
}
