//! Painting text laid out by Parley.

use anyrender::PaintScene;
use anyrender::filters::{Filter, FilterEffect};
use blitz_dom::node::Marker;
use blitz_dom::text::InlineText as _;
use blitz_dom::text::parley::parley::{GlyphRun, Line, PositionedLayoutItem};
use blitz_dom::node::TextLayout;
use blitz_dom::text::parley::{MarkerLayout, TextBrush, TextEditor};
use blitz_dom::{BaseDocument, CopyStyleGetters, NodeId};
use kurbo::{Affine, Point, Rect};
use peniko::{Fill, Mix};
use std::sync::Arc;

use super::{DrawTextContext, RunRecord, TextPaintOptions, font_embolden, paint_selection};
use crate::color::{Color, ToColorColor as _};
use crate::text_decorations::{
    DecorationRunGeometry, LineDecoration, ResolvedDecoration, WinAscentCache,
    flush_line_decorations, resolve_decoration_entry,
};
use style::properties::generated::longhands::text_decoration_style::computed_value::T as TextDecorationStyle;
use style::values::computed::TextDecorationLine;
use style::values::generics::text::{GenericTextDecorationInset, GenericTextDecorationLength};
use crate::{FONT_EMBOLDEN_ENABLED, SELECTION_COLOR};


/// Paints an inline formatting context: its inline elements' backgrounds, the selection
/// highlight, and its text and decorations.
///
/// `transform` takes the content box's device pixels, moved down by the layout's block offset,
/// onto the scene. `content_size` is the content box's size in device pixels, which every text
/// backend's painter is handed and Parley's does not need.
///
/// Parley's painter leaves atomic inlines to be painted after the text, so `_paint_atomic` is
/// never called.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_inline_layout<S: PaintScene>(
    scene: &mut S,
    text_layout: &TextLayout,
    doc: &BaseDocument,
    transform: Affine,
    options: TextPaintOptions,
    root: NodeId,
    selection: Option<(usize, usize)>,
    context: &std::cell::RefCell<DrawTextContext>,
    _paint_atomic: &mut impl FnMut(&mut S, NodeId),
) {
    let context = &mut *context.borrow_mut();
    // Render inline element backgrounds (e.g. `<span style="background: ...">`)
    // behind the text and selection highlight.
    draw_inline_backgrounds(scene, text_layout.backend.layout.lines(), doc, transform, root);

    // Render text selection highlight (if any) using cached selection ranges
    if let Some((sel_start, sel_end)) = selection {
        paint_selection(scene, text_layout, transform, sel_start, sel_end);
    }

    // Render text
    stroke_text(
        scene,
        text_layout.backend.layout.lines(),
        doc,
        transform,
        options,
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
    options: TextPaintOptions,
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
    let Some(layout) = editor.try_layout() else {
        return;
    };
    stroke_text(
        scene,
        layout.lines(),
        doc,
        transform,
        options,
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
    options: TextPaintOptions,
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
    let y_offset = text_layout
        .and_then(|text_layout| text_layout.first_baseline())
        .zip(layout.lines().next())
        .map_or(0.0, |(baseline, marker_line)| {
            baseline - marker_line.metrics().baseline / layout.scale()
        });

    let pos = Point {
        x: pos.x + x_offset as f64,
        y: pos.y + y_offset as f64,
    };

    let transform = transform * Affine::translate((pos.x * options.scale, pos.y * options.scale));

    stroke_text(
        scene,
        layout.lines(),
        doc,
        transform,
        options,
        node_id,
        context,
    );
}

/// Converts parley BoundingBox into peniko Rect
fn convert_rect(rect: &blitz_dom::text::parley::parley::BoundingBox) -> kurbo::Rect {
    peniko::kurbo::Rect::new(rect.x0, rect.y0, rect.x1, rect.y1)
}

/// Paints only the text of an inline formatting context, for a `background-clip: text` mask.
///
/// `transform` takes the content box's device pixels, moved down by the layout's block offset,
/// onto the scene.
pub(crate) fn paint_inline_text(
    scene: &mut impl PaintScene,
    text_layout: &TextLayout,
    doc: &BaseDocument,
    transform: Affine,
    options: TextPaintOptions,
    root: NodeId,
    context: &mut DrawTextContext,
) {
    stroke_text(
        scene,
        text_layout.backend.layout.lines(),
        doc,
        transform,
        options,
        root,
        context,
    );
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
    options: TextPaintOptions,
    inline_root_id: NodeId,
    context: &mut DrawTextContext,
) {
    let TextPaintOptions {
        scale,
        brush_override,
    } = options;
    let DrawTextContext {
        stack,
        path_scratch,
        deco_boxes,
        run_records,
        win_ascent_ratios,
    } = context;
    stack.clear();
    path_scratch.clear();
    deco_boxes.clear();
    run_records.clear();

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
        run_records.clear();
        let mut line_has_shadows = false;
        // The composing text's underline on this line, spanning its runs.
        let mut composing: Option<LineDecoration> = None;

        // Pass 1: resolve styles for every glyph run on the line (recording the values
        // needed to paint it) and accumulate decorations. Painting is deferred so text
        // shadows can be drawn beneath *all* of the line's text and decorations.
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let run = glyph_run.run();
            let font = run.font();
            let font_size = run.font_size();
            let metrics = run.font_metrics();
            let style = glyph_run.style();

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
                let entry = resolve_decoration_entry(doc, node_id, stack.last(), scale);
                stack.push(entry);
            }

            // The glyph colour and `text-shadow` come from the run's own node (the stack
            // top): both inherit, so the innermost inline element already carries the
            // right values. Shadows are not part of a `background-clip: text` mask, so
            // they are dropped when the brush is overridden.
            let (fill_color, shadows) = stack
                .last()
                .map(|e| (e.fill_color, e.text_shadows.clone()))
                .unwrap_or((Color::BLACK, None));
            let fill_color = brush_override.unwrap_or(fill_color);
            let shadows = shadows.filter(|_| brush_override.is_none());
            line_has_shadows |= shadows.is_some();
            run_records.push(RunRecord {
                fill_color,
                shadows,
            });

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
                font: Some(font.clone()),
                font_size,
                css_font_size,
                centered: false,
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
                            piece: None,
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

                // A text input's editor underlines the text an input method is composing.
                if style.underline.is_some() {
                    let acc = composing.get_or_insert_with(|| LineDecoration {
                        node_id: run_node_id,
                        deco: composition_underline(fill_color),
                        min_x: f64::INFINITY,
                        max_x: f64::NEG_INFINITY,
                        piece: None,
                        own: None,
                        first: None,
                    });
                    acc.min_x = acc.min_x.min(run_x0);
                    acc.max_x = acc.max_x.max(run_x1);
                    acc.first.get_or_insert(geometry);
                }
        }

        // Pass 2: text shadows, beneath all of the line's text and decorations.
        if line_has_shadows {
            draw_line_text_shadows(
                scene,
                &line,
                run_records,
                deco_boxes,
                win_ascent_ratios,
                transform,
                options,
                inline_root_id,
            );
        }

        // Pass 3: the glyphs themselves.
        for (glyph_run, record) in glyph_runs(&line).zip(run_records.iter()) {
            draw_glyph_run(scene, &glyph_run, record.fill_color, transform);
        }

        // Pass 4: decorations, on top of the glyphs.
        flush_line_decorations(
            scene,
            transform,
            options,
            deco_boxes,
            win_ascent_ratios,
            Some((inline_root_id, line.metrics().baseline)),
            TextDecorationLine::all(),
            None,
        );
        if let Some(composing) = composing.take() {
            flush_line_decorations(
                scene,
                transform,
                options,
                std::slice::from_ref(&composing),
                win_ascent_ratios,
                Some((inline_root_id, line.metrics().baseline)),
                TextDecorationLine::all(),
                None,
            );
        }
    }
}

/// The underline under the text an input method is composing: solid, `auto` thick, and in
/// the text's colour.
fn composition_underline(color: Color) -> ResolvedDecoration {
    ResolvedDecoration {
        line: TextDecorationLine::UNDERLINE,
        style: TextDecorationStyle::Solid,
        color,
        thickness: GenericTextDecorationLength::Auto,
        underline_offset: None,
        underline_under: false,
        underline_from_font: false,
        inset: GenericTextDecorationInset::Auto,
    }
}

/// The glyph runs of a line, in order (matching the order of the recorded `RunRecord`s).
pub(crate) fn glyph_runs<'a, 'b>(
    line: &'b Line<'a, TextBrush>,
) -> impl Iterator<Item = GlyphRun<'a, TextBrush>> + 'b {
    line.items().filter_map(|item| match item {
        PositionedLayoutItem::GlyphRun(glyph_run) => Some(glyph_run),
        PositionedLayoutItem::InlineBox(_) => None,
    })
}

/// Draw a glyph run's glyphs in a single colour.
pub(crate) fn draw_glyph_run(
    scene: &mut impl PaintScene,
    glyph_run: &GlyphRun<'_, TextBrush>,
    color: Color,
    transform: Affine,
) {
    let run = glyph_run.run();
    let font_size = run.font_size();
    // Zero-size text is invisible, but hinting can give its glyphs ink. Fully
    // transparent text is skipped too: backends draw color (emoji) glyphs opaque.
    if font_size <= 0.0 || color.components[3] <= 0.0 {
        return;
    }
    let glyph_xform = run
        .synthesis()
        .skew()
        .map(|angle| Affine::skew(angle.to_radians().tan() as f64, 0.0));

    let embolden = if FONT_EMBOLDEN_ENABLED {
        font_embolden(font_size as f64, color, transform)
    } else {
        kurbo::Vec2::default()
    };

    let normalized_coords: &[i16] = bytemuck::cast_slice(run.normalized_coords());
    scene.draw_glyphs(
        run.font(),
        font_size,
        !FONT_EMBOLDEN_ENABLED, // hint
        normalized_coords,
        embolden,
        Fill::NonZero,
        &anyrender::Paint::from(color),
        1.0, // alpha
        transform,
        glyph_xform,
        glyph_run.positioned_glyphs().map(|glyph| anyrender::Glyph {
            id: glyph.id as _,
            x: glyph.x,
            y: glyph.y,
        }),
    );
}

/// The layout-space box covered by a glyph run: its advance horizontally and the font's
/// ascent/descent vertically.
pub(crate) fn glyph_run_rect(glyph_run: &GlyphRun<'_, TextBrush>) -> Rect {
    let metrics = glyph_run.run().font_metrics();
    let x0 = glyph_run.offset() as f64;
    let x1 = x0 + glyph_run.advance() as f64;
    let baseline = glyph_run.baseline() as f64;
    let y0 = baseline - metrics.ascent as f64;
    let y1 = baseline + metrics.descent as f64;
    Rect::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1))
}

/// Paint the `text-shadow`s of one line.
///
/// Like Blink and Gecko, a shadow is painted by drawing the text again, offset and in the
/// shadow colour, beneath the real text; the shadows in a list are drawn last-to-first so
/// the first one ends up on top. An unblurred shadow is just an extra glyph draw. A
/// blurred one needs a blur filter layer, which is the expensive part, so consecutive runs
/// that share the same `text-shadow` list (by `Arc` identity: a paragraph's runs normally
/// all inherit one list) are batched into a single layer per shadow.
#[allow(clippy::too_many_arguments)]
fn draw_line_text_shadows(
    scene: &mut impl PaintScene,
    line: &Line<'_, TextBrush>,
    run_records: &[RunRecord],
    deco_boxes: &[LineDecoration],
    win_ascent_ratios: &mut WinAscentCache,
    transform: Affine,
    options: TextPaintOptions,
    inline_root_id: NodeId,
) {
    let line_metrics = line.metrics();
    let line_baseline = line_metrics.baseline;
    let line_height = (line_metrics.block_max_coord - line_metrics.block_min_coord) as f64;

    let mut start = 0;
    while start < run_records.len() {
        let Some(shadow_list) = &run_records[start].shadows else {
            start += 1;
            continue;
        };

        // The batch: this run plus following runs with the same shadow list.
        let end = start
            + run_records[start..]
                .iter()
                .take_while(|record| {
                    record
                        .shadows
                        .as_ref()
                        .is_some_and(|list| Arc::ptr_eq(list, shadow_list))
                })
                .count();
        let batch = || glyph_runs(line).skip(start).take(end - start);

        let glyph_bounds = batch()
            .map(|run| glyph_run_rect(&run))
            .reduce(|a, b| a.union(b))
            .unwrap_or(Rect::ZERO);
        // Decorations drawn under these runs are shadowed with them, clipped to the runs'
        // horizontal extent. The vertical extent is generous to cover any decoration style.
        let has_decorations = deco_boxes
            .iter()
            .any(|acc| acc.max_x > glyph_bounds.x0 && acc.min_x < glyph_bounds.x1);
        let deco_clip = Rect::new(
            glyph_bounds.x0,
            line_metrics.block_min_coord as f64 - line_height,
            glyph_bounds.x1,
            line_metrics.block_max_coord as f64 + line_height,
        );
        let batch_bounds = if has_decorations {
            glyph_bounds.union(deco_clip)
        } else {
            glyph_bounds
        };

        for shadow in shadow_list.iter().rev() {
            if shadow.color.components[3] <= 0.0 {
                continue;
            }
            if shadow.is_blurred() {
                let filter = Arc::new(Filter::single(FilterEffect::blur(shadow.std_dev as f32)));
                // Layer clip: the shadowed content's bounds, offset, plus room for the blur.
                let margin = 3.0 * shadow.std_dev;
                let clip = (batch_bounds + shadow.offset).inflate(margin, margin);
                scene.push_layer(
                    Fill::NonZero,
                    Mix::Normal,
                    1.0,
                    transform,
                    &clip,
                    Some(filter),
                    None,
                );
            }

            let shadow_transform = transform * Affine::translate(shadow.offset);
            for run in batch() {
                draw_glyph_run(scene, &run, shadow.color, shadow_transform);
            }
            if has_decorations {
                flush_line_decorations(
                    scene,
                    transform,
                    options,
                    deco_boxes,
                    win_ascent_ratios,
                    Some((inline_root_id, line_baseline)),
                    TextDecorationLine::all(),
                    Some((shadow, deco_clip)),
                );
            }

            if shadow.is_blurred() {
                scene.pop_layer();
            }
        }

        start = end;
    }
}
