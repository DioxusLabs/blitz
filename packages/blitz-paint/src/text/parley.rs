//! Painting text laid out by Parley.

use anyrender::PaintScene;
use blitz_dom::node::Marker;
use blitz_dom::text::InlineText as _;
use blitz_dom::text::parley::parley::{
    Affinity, Cursor, Layout, Line, PositionedLayoutItem, Selection,
};
use blitz_dom::text::parley::{MarkerLayout, TextBrush, TextEditor, TextLayout};
use blitz_dom::{BaseDocument, NodeId};
use kurbo::{Affine, Point, Rect};
use peniko::Fill;

use super::{
    DecorationRunGeometry, DrawTextContext, LineDecoration, flush_line_decorations,
    resolve_decoration_entry,
};
use crate::color::{Color, ToColorColor as _};
use crate::{FONT_EMBOLDEN_ENABLED, SELECTION_COLOR};

/// Paints an inline formatting context: its inline elements' backgrounds, the selection
/// highlight, and its text and decorations.
///
/// `transform` takes the content box's device pixels, moved down by the layout's block offset,
/// onto the scene.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_inline_layout(
    scene: &mut impl PaintScene,
    text_layout: &TextLayout,
    doc: &BaseDocument,
    transform: Affine,
    scale: f64,
    root: NodeId,
    selection: Option<(usize, usize)>,
    context: &mut DrawTextContext,
) {
    // Render inline element backgrounds (e.g. `<span style="background: ...">`)
    // behind the text and selection highlight.
    draw_inline_backgrounds(scene, text_layout.layout.lines(), doc, transform, root);

    // Render text selection highlight (if any) using cached selection ranges
    if let Some((sel_start, sel_end)) = selection {
        draw_text_selection(scene, &text_layout.layout, transform, sel_start, sel_end);
    }

    // Render text
    stroke_text(
        scene,
        text_layout.layout.lines(),
        doc,
        transform,
        scale,
        root,
        context,
    );
}

/// Paints the text of an `<input>` or `<textarea>`, and its selection and caret where it is
/// focussed.
///
/// `transform` takes the text's device pixels onto the scene.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_text_input(
    scene: &mut impl PaintScene,
    editor: &TextEditor,
    focussed: bool,
    caret_color: Color,
    doc: &BaseDocument,
    transform: Affine,
    scale: f64,
    node_id: NodeId,
    context: &mut DrawTextContext,
) {
    let editor = editor.plain_editor();
    if focussed {
        // Render selection/caret
        for (rect, _line_idx) in editor.selection_geometry().iter() {
            scene.fill(
                Fill::NonZero,
                transform,
                SELECTION_COLOR,
                None,
                &convert_rect(rect),
            );
        }
        if let Some(cursor) = editor.cursor_geometry(1.5) {
            scene.fill(
                Fill::NonZero,
                transform,
                caret_color,
                None,
                &convert_rect(&cursor),
            );
        };
    }

    // Render text
    stroke_text(
        scene,
        editor.try_layout().unwrap().lines(),
        doc,
        transform,
        scale,
        node_id,
        context,
    );
}

/// Paints a list item's outside marker, right-aligned before its border box, on the baseline of
/// the item's first line of text.
///
/// `pos` is the item's content box origin in CSS pixels, and `transform` takes the element's CSS
/// pixels, scaled to device pixels, onto the scene.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_marker(
    scene: &mut impl PaintScene,
    marker: &Marker,
    layout: &MarkerLayout,
    text_layout: Option<&TextLayout>,
    item_layout: &taffy::Layout,
    doc: &BaseDocument,
    pos: Point,
    transform: Affine,
    scale: f64,
    node_id: NodeId,
    context: &mut DrawTextContext,
) {
    // Right align and pad the bullet when rendering outside
    let x_padding = match marker {
        Marker::Char(_) => 8.0,
        Marker::String(_) => 0.0,
    };
    // Outside markers are placed outside the list item's border box
    // (`pos` is the origin of its content box)
    let x_offset = -(layout.full_width() / layout.scale()
        + x_padding
        + item_layout.padding.left
        + item_layout.border.left);

    // Align the marker with the baseline of the first line of text in the list item
    let y_offset = if let Some((text_layout, first_text_line)) = &text_layout
        .and_then(|text_layout| Some((text_layout, text_layout.layout.lines().next()?)))
    {
        (first_text_line.metrics().baseline - layout.lines().next().unwrap().metrics().baseline)
            / layout.scale()
            + text_layout.block_offset()
    } else {
        0.0
    };

    let pos = Point {
        x: pos.x + x_offset as f64,
        y: pos.y + y_offset as f64,
    };

    let transform = transform * Affine::translate((pos.x * scale, pos.y * scale));

    stroke_text(
        scene,
        layout.lines(),
        doc,
        transform,
        scale,
        node_id,
        context,
    );
}

/// Converts parley BoundingBox into peniko Rect
fn convert_rect(rect: &blitz_dom::text::parley::parley::BoundingBox) -> kurbo::Rect {
    peniko::kurbo::Rect::new(rect.x0, rect.y0, rect.x1, rect.y1)
}

/// Draw the backgrounds of inline elements (e.g. `<span style="background: ...">`).
///
/// Each glyph run carries the node id of the innermost inline element it belongs to
/// (via its brush). We look up that node's `background-color` and, if non-transparent,
/// fill a rectangle covering the run's advance and its font's ascent/descent so that the
/// background sits behind the text.
///
/// The inline root's own background is painted separately (as a normal block box), so
/// runs belonging to the root are skipped to avoid drawing it twice.
fn draw_inline_backgrounds<'a>(
    scene: &mut impl PaintScene,
    lines: impl Iterator<Item = Line<'a, TextBrush>>,
    doc: &BaseDocument,
    transform: Affine,
    inline_root_id: NodeId,
) {
    for line in lines {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };

            let node_id = glyph_run.style().brush.id;
            if node_id == inline_root_id {
                continue;
            }

            let Some(styles) = doc.get_node(node_id).and_then(|node| node.primary_styles()) else {
                continue;
            };

            let current_color = styles.clone_color();
            let bg_color = styles
                .get_background()
                .background_color
                .resolve_to_absolute(&current_color)
                .as_srgb_color();
            if bg_color == Color::TRANSPARENT {
                continue;
            }

            let metrics = glyph_run.run().font_metrics();
            let x = glyph_run.offset() as f64;
            let w = glyph_run.advance() as f64;
            let baseline = glyph_run.baseline() as f64;
            let y0 = baseline - metrics.ascent as f64;
            let y1 = baseline + metrics.descent as f64;
            let rect = Rect::new(x, y0, x + w, y1);

            scene.fill(Fill::NonZero, transform, bg_color, None, &rect);
        }
    }
}

fn stroke_text<'a>(
    scene: &mut impl PaintScene,
    lines: impl Iterator<Item = Line<'a, TextBrush>>,
    doc: &BaseDocument,
    transform: Affine,
    scale: f64,
    inline_root_id: NodeId,
    context: &mut DrawTextContext,
) {
    let DrawTextContext {
        stack,
        path_scratch,
        deco_boxes,
        win_ascent_ratios,
    } = context;
    stack.clear();
    path_scratch.clear();
    deco_boxes.clear();

    // Persistent stack mirroring the ancestor path (inline root -> current run's
    // node) as we walk the runs. The `text-decoration-*` properties are *not*
    // inherited; instead a decoration set on an ancestor is propagated to the
    // descendant text it wraps. Rather than re-resolving styles for the whole
    // ancestor chain on every run, we cache each node's resolved values here and,
    // for each run, only resolve styles for the nodes newly descended into (popping
    // as we ascend). `path_scratch` is a reusable buffer for the run's node path.
    for line in lines {
        // Decorations accumulated for this line, keyed by decorating box, so each box is
        // painted once (spanning all its runs) using its own font — matching Firefox, which
        // draws one decoration per box rather than one stepped segment per differently-sized
        // run. Clearing preserves the allocation for the next line and inline context.
        deco_boxes.clear();

        for item in line.items() {
            if let PositionedLayoutItem::GlyphRun(glyph_run) = item {
                let run = glyph_run.run();
                let font = run.font();
                let font_size = run.font_size();
                let metrics = run.font_metrics();
                let style = glyph_run.style();
                let synthesis = run.synthesis();
                let glyph_xform = synthesis
                    .skew()
                    .map(|angle| Affine::skew(angle.to_radians().tan() as f64, 0.0));

                let css_font_size = font_size as f64 / scale;

                // Reconcile the stack with this run's ancestor path. Build the path
                // (inline root -> run node), keep the shared prefix already on the stack,
                // pop the rest, and resolve styles only for the newly-descended nodes.
                path_scratch.clear();
                let mut walk_id = Some(style.brush.id);
                while let Some(node_id) = walk_id {
                    path_scratch.push(node_id);
                    if node_id == inline_root_id {
                        break;
                    }
                    walk_id = doc.get_node(node_id).and_then(|node| node.parent);
                }
                path_scratch.reverse();

                let shared = stack
                    .iter()
                    .zip(path_scratch.iter())
                    .take_while(|(entry, node_id)| entry.node_id == **node_id)
                    .count();
                stack.truncate(shared);
                for &node_id in &path_scratch[shared..] {
                    stack.push(resolve_decoration_entry(doc, node_id));
                }

                // The glyph colour comes from the run's own node (the stack top): `color`
                // inherits, so the innermost inline element already carries the right value.
                let text_color = stack.last().map(|e| e.text_color).unwrap_or(Color::BLACK);

                let embolden = if FONT_EMBOLDEN_ENABLED {
                    let fs = font_size as f64 / scale;
                    kurbo::Vec2::new((0.015125 * fs).min(0.3), (0.0121 * fs).min(0.3))
                } else {
                    kurbo::Vec2::default()
                };

                let normalized_coords: &[i16] = bytemuck::cast_slice(run.normalized_coords());
                scene.draw_glyphs(
                    font,
                    font_size,
                    !FONT_EMBOLDEN_ENABLED, // hint
                    normalized_coords,
                    embolden,
                    Fill::NonZero,
                    &anyrender::Paint::from(text_color),
                    1.0, // alpha
                    transform,
                    glyph_xform,
                    glyph_run.positioned_glyphs().map(|glyph| anyrender::Glyph {
                        id: glyph.id as _,
                        x: glyph.x,
                        y: glyph.y,
                    }),
                );

                // Accumulate this run's contribution to each decorating box on its ancestor
                // path. The decoration is drawn once per box after the whole line has been
                // walked (see `flush_line_decorations`), so mixed font sizes within a box
                // produce a single straight line rather than one stepped segment per run.
                let geometry = DecorationRunGeometry {
                    baseline: glyph_run.baseline(),
                    ascent: metrics.ascent,
                    descent: metrics.descent,
                    underline_offset: metrics.underline_offset,
                    underline_size: metrics.underline_size,
                    strikethrough_size: metrics.strikethrough_size,
                    font: font.clone(),
                    font_size,
                    css_font_size,
                };
                let run_node_id = style.brush.id;
                let run_x0 = glyph_run.offset() as f64;
                let run_x1 = run_x0 + glyph_run.advance() as f64;

                for entry in stack.iter() {
                    if entry.decoration.is_none() {
                        continue;
                    }
                    let idx = match deco_boxes.iter().position(|d| d.node_id == entry.node_id) {
                        Some(idx) => idx,
                        None => {
                            deco_boxes.push(LineDecoration {
                                node_id: entry.node_id,
                                deco: entry.decoration.clone().unwrap(),
                                min_x: f64::INFINITY,
                                max_x: f64::NEG_INFINITY,
                                own: None,
                                first: None,
                            });
                            deco_boxes.len() - 1
                        }
                    };
                    let acc = &mut deco_boxes[idx];
                    acc.min_x = acc.min_x.min(run_x0);
                    acc.max_x = acc.max_x.max(run_x1);
                    if acc.first.is_none() {
                        acc.first = Some(geometry.clone());
                    }
                    // A run whose innermost node is the box itself is the box's own text, so
                    // its font is the one Firefox positions and sizes the decoration with.
                    if entry.node_id == run_node_id {
                        acc.own = Some(geometry.clone());
                    }
                }
            }
        }

        flush_line_decorations(
            scene,
            transform,
            scale,
            deco_boxes,
            win_ascent_ratios,
            inline_root_id,
            line.metrics().baseline,
        );
    }
}

/// Draw selection highlight rectangles for the given byte range in a layout.
/// Uses Parley's Selection type for accurate geometry calculation.
fn draw_text_selection(
    scene: &mut impl PaintScene,
    layout: &Layout<TextBrush>,
    transform: Affine,
    selection_start: usize,
    selection_end: usize,
) {
    let anchor = Cursor::from_byte_index(layout, selection_start, Affinity::Downstream);
    let focus = Cursor::from_byte_index(layout, selection_end, Affinity::Downstream);
    let selection = Selection::new(anchor, focus);

    selection.geometry_with(layout, |rect, _line_idx| {
        let rect = kurbo::Rect::new(rect.x0, rect.y0, rect.x1, rect.y1);
        scene.fill(Fill::NonZero, transform, SELECTION_COLOR, None, &rect);
    });
}
