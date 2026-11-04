use crate::color::ToColorColor;
use anyrender::{PaintScene, Scene};
use blitz_dom::{BaseDocument, CopyStyleGetters};
use kurbo::{Affine, Rect, Vec2};
use parley::layout::PositionedLayoutItem;
use peniko::{Fill, color::AlphaColor};

pub(crate) fn build_select_overlay_scenes(
    scenes: &mut std::collections::HashMap<usize, Scene>,
    doc: &mut BaseDocument,
    scale: f64,
) {
    if doc.open_select.is_some() {
        let mut scene = Scene::new();
        paint_select_overlay(&mut scene, doc, scale);
        scenes.insert(doc.id(), scene);
    }
    for node_id in doc.sub_document_node_ids() {
        if let Some(sub_doc) = doc.get_node_mut(node_id).and_then(|node| node.subdoc_mut()) {
            build_select_overlay_scenes(scenes, &mut sub_doc.inner_mut(), scale);
        }
    }
}

fn paint_select_overlay(scene: &mut impl PaintScene, doc: &mut BaseDocument, scale: f64) {
    let Some(overlay) = doc.select_overlay() else {
        return;
    };
    let transform = Affine::scale(scale);
    let frame = Rect::new(
        overlay.rect.x,
        overlay.rect.y,
        overlay.rect.x + overlay.rect.width,
        overlay.rect.y + overlay.rect.height,
    );
    scene.fill(
        Fill::NonZero,
        transform,
        AlphaColor::from_rgba8(0, 0, 0, 35),
        None,
        &(frame + Vec2::new(2.0, 3.0)),
    );
    scene.fill(
        Fill::NonZero,
        transform,
        AlphaColor::from_rgb8(140, 140, 140),
        None,
        &frame,
    );
    scene.fill(
        Fill::NonZero,
        transform,
        AlphaColor::WHITE,
        None,
        &frame.inset(-1.0),
    );
    let color = doc
        .get_node(overlay.select)
        .and_then(|n| n.primary_styles())
        .map(|s| s.clone_color().as_srgb_color())
        .unwrap_or(AlphaColor::BLACK);
    scene.push_clip_layer(Fill::NonZero, transform, &frame.inset(-1.0));
    for row in overlay.rows {
        let rect = Rect::new(
            row.rect.x,
            row.rect.y,
            row.rect.x + row.rect.width,
            row.rect.y + row.rect.height,
        );
        let text_color = if row.disabled {
            AlphaColor::from_rgb8(140, 140, 140)
        } else if row.highlighted {
            AlphaColor::WHITE
        } else {
            color
        };
        if row.highlighted {
            scene.fill(
                Fill::NonZero,
                transform,
                AlphaColor::from_rgb8(38, 98, 190),
                None,
                &rect,
            );
        }
        let text_transform = transform
            * Affine::translate((
                row.rect.x + 10.0,
                row.rect.y + (row.rect.height - row.layout.height() as f64) / 2.0,
            ));
        for line in row.layout.lines() {
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(glyphs) = item {
                    let run = glyphs.run();
                    scene.draw_glyphs(
                        run.font(),
                        run.font_size(),
                        true,
                        bytemuck::cast_slice(run.normalized_coords()),
                        Vec2::ZERO,
                        Fill::NonZero,
                        &anyrender::Paint::from(text_color),
                        1.0,
                        text_transform,
                        None,
                        glyphs.positioned_glyphs().map(|g| anyrender::Glyph {
                            id: g.id as _,
                            x: g.x,
                            y: g.y,
                        }),
                    );
                }
            }
        }
    }
    scene.pop_layer();
}
