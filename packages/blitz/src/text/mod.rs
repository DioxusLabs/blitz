use std::cell::RefCell;

use blitz_dom::node::TextLayout;
use parley::layout::LayoutItem2;
use vello::{glyph::skrifa::raw::FileRef, skrifa::prelude::*, Scene};
use vello::{
    glyph::GlyphContext,
    kurbo::Affine,
    peniko::{Brush, Fill, Font},
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
        text_layout: &TextLayout,
    ) {
        let brush = brush.map(Into::into).unwrap_or_default();

        // let font = font.and_then(to_font_ref).unwrap_or_else(default_font);
        // let fello_size = Size::new(size);
        // let charmap = font.charmap();
        // let metrics = font.metrics(fello_size, LocationRef::default());
        // let line_height = metrics.ascent - metrics.descent + metrics.leading;
        // let glyph_metrics = font.glyph_metrics(fello_size, LocationRef::default());
        // let mut pen_x = 0f64;
        // let mut pen_y = 0f64;
        // let vars: [(&str, f32); 0] = [];

        // let mut gcx = self.gcx.borrow_mut();

        // let mut provider = gcx.new_provider(&font, size, false, vars);
        // let mut provider = gcx.new_provider(&font, None, size, false, vars);

        // for ch in text.chars() {
        //     if ch == '\n' {
        //         pen_y += line_height as f64;
        //         pen_x = 0.0;
        //         continue;
        //     }
        //     let gid = charmap.map(ch).unwrap_or_default();
        //     let advance = glyph_metrics.advance_width(gid).unwrap_or_default() as f64;
        //     if let Some(glyph) = provider.get(gid.to_u16(), brush.as_ref()) {
        //         let xform = transform
        //             * Affine::translate((pen_x, pen_y))
        //             * Affine::scale_non_uniform(1.0, -1.0);
        //         builder.append(&glyph, Some(xform));
        //     }
        //     pen_x += advance;
        // }

        for line in text_layout.layout.lines() {
            for item in line.items() {
                if let LayoutItem2::GlyphRun(glyph_run) = item {
                    let mut x = glyph_run.offset();
                    let y = glyph_run.baseline();
                    let run = glyph_run.run();
                    let font = run.font();
                    let font_size = run.font_size();
                    let style = glyph_run.style();
                    let synthesis = run.synthesis();
                    let glyph_xform = synthesis
                        .skew()
                        .map(|angle| Affine::skew(angle.to_radians().tan() as f64, 0.0));
                    let coords = run
                        .normalized_coords()
                        .iter()
                        .map(|coord| vello::skrifa::instance::NormalizedCoord::from_bits(*coord))
                        .collect::<Vec<_>>();

                    builder
                        .draw_glyphs(font)
                        .brush(&style.brush.color)
                        .transform(transform)
                        .glyph_transform(glyph_xform)
                        .font_size(font_size)
                        .normalized_coords(&coords)
                        .draw(
                            Fill::NonZero,
                            glyph_run.glyphs().map(|glyph| {
                                let gx = x + glyph.x;
                                let gy = y - glyph.y;
                                x += glyph.advance;
                                vello::glyph::Glyph {
                                    id: glyph.id as _,
                                    x: gx,
                                    y: gy,
                                }
                            }),
                        );
                }
            }
        }
    }

    // pub fn get_text_size<'a>(
    //     &'a self,
    //     font: Option<&'a Font>,
    //     size: f32,
    //     text: &str,
    // ) -> (f64, f64) {
    //     let font = font.and_then(to_font_ref).unwrap_or_else(default_font);
    //     let fello_size = Size::new(size);
    //     let charmap = font.charmap();
    //     let metrics = font.metrics(fello_size, LocationRef::default());
    //     let line_height = metrics.ascent - metrics.descent + metrics.leading;
    //     let glyph_metrics = font.glyph_metrics(fello_size, LocationRef::default());
    //     let mut max_width = 0;
    //     let mut cur_width = 0f64;
    //     let mut height = line_height as f64;

    //     for ch in text.chars() {
    //         if ch == '\n' {
    //             height += line_height as f64;
    //             max_width = max_width.max(cur_width as i32);
    //             cur_width = 0.0;
    //             continue;
    //         }
    //         let gid = charmap.map(ch).unwrap_or_default();
    //         let advance = glyph_metrics.advance_width(gid).unwrap_or_default() as f64;
    //         cur_width += advance;
    //     }

    //     max_width = max_width.max(cur_width as i32);

    //     (max_width as _, height)
    // }
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
