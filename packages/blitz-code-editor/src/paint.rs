use anyrender::PaintScene;
use floem_editor_core::buffer::rope_text::RopeText;
use kurbo::{Affine, Point, Rect, Size, Vec2};
use parley::PositionedLayoutItem;
use peniko::{Color, Fill};

use crate::layout::{TextLayout, caret_point, line_range, range_rects};
use crate::{CodeEditor, TextContext};

const CARET_WIDTH: f64 = 2.0;

impl CodeEditor {
    /// Paints the editor. `transform` maps editor coordinates (logical pixels, origin at the
    /// editor's top-left corner) to the scene, e.g. `Affine::scale(scale_factor)`.
    pub fn paint(&mut self, cx: &mut TextContext, scene: &mut impl PaintScene, transform: Affine) {
        self.prepare(cx);
        let lh = self.metrics.line_height;
        let bounds = Rect::from_origin_size(Point::ZERO, self.size);
        let gutter_width = self.gutter_width();
        let text_x = self.text_x() - self.scroll.x;
        let theme = self.theme.clone();
        let selection = self.selection();

        scene.fill(Fill::NonZero, transform, theme.background, None, &bounds);
        scene.push_clip_layer(Fill::NonZero, transform, &bounds);

        let last_line = self.buffer.last_line();
        let (first_visible, _) = self.cache.rows().line_at((self.scroll.y / lh) as usize);
        let caret_line = self.buffer.line_of_offset(selection.get_cursor_offset());
        // The visible lines, with their tops in editor coordinates and whether a caret is on them
        let mut visible = Vec::new();

        for line in first_visible..=last_line {
            // Laying the line out may change its height, but not its top
            let y = self.line_top(line) - self.scroll.y;
            if y >= self.size.height {
                break;
            }
            let range = line_range(&self.buffer, line);
            let next_line_start = self.buffer.offset_of_line(line + 1);
            let line_transform = transform * Affine::translate(Vec2::new(text_x, y));

            // Lay out the line, splicing in IME composition text at the primary caret
            let preedit = self
                .preedit
                .as_ref()
                .filter(|_| self.focused && line == caret_line)
                .map(|p| (selection.get_cursor_offset() - range.start, p.clone()));
            let preedit_layout = preedit.as_ref().map(|(col, p)| {
                self.cache
                    .line_with_preedit(cx, &self.buffer, line, *col, &p.text)
            });
            let layout: &TextLayout = match &preedit_layout {
                Some(layout) => layout,
                None => self.cache.line(cx, &self.buffer, line),
            };

            let regions_on_line = selection
                .regions()
                .iter()
                .filter(|r| r.min() <= range.end && r.max() >= range.start);
            // The caret positions on this line (in layout coordinates)
            let carets: Vec<Point> = regions_on_line
                .clone()
                .filter(|r| r.end >= range.start && r.end <= range.end)
                .map(|region| {
                    let mut col = region.end - range.start;
                    if let Some((preedit_col, p)) = &preedit
                        && region.end == selection.get_cursor_offset()
                    {
                        col = preedit_col + p.cursor.map_or(p.text.len(), |c| c.1);
                    }
                    caret_point(layout, col)
                })
                .collect();

            // Highlight the row of each collapsed caret
            let has_caret = regions_on_line.clone().any(|r| r.is_caret());
            for region in regions_on_line.clone().filter(|r| r.is_caret()) {
                let row_y = caret_point(layout, region.end - range.start).y + y;
                let rect = Rect::new(gutter_width, row_y, self.size.width, row_y + lh);
                scene.fill(Fill::NonZero, transform, theme.current_line, None, &rect);
            }

            // Selection highlights
            for region in regions_on_line.filter(|r| !r.is_caret()) {
                let start = region.min().max(range.start) - range.start;
                let end = region.max().min(range.end) - range.start;
                for rect in range_rects(layout, start..end) {
                    let rect = Rect::new(rect.x0, rect.y0, rect.x1, rect.y1);
                    scene.fill(Fill::NonZero, line_transform, theme.selection, None, &rect);
                }
                // Show a selected line ending as a space-wide block
                if region.max() >= next_line_start && next_line_start > range.end {
                    let origin = caret_point(layout, end);
                    let size = Size::new(self.metrics.char_width, lh);
                    let rect = Rect::from_origin_size(origin, size);
                    scene.fill(Fill::NonZero, line_transform, theme.selection, None, &rect);
                }
            }

            draw_layout(scene, layout, line_transform, None);

            if let Some((col, p)) = &preedit {
                let end = col + p.text.len();
                for rect in range_rects(layout, *col..end) {
                    let rect = Rect::new(rect.x0, rect.y1 - 2.0, rect.x1, rect.y1 - 1.0);
                    scene.fill(Fill::NonZero, line_transform, theme.foreground, None, &rect);
                }
            }

            if self.focused && self.caret_visible {
                for origin in carets {
                    let rect = Rect::from_origin_size(origin, Size::new(CARET_WIDTH, lh));
                    scene.fill(Fill::NonZero, line_transform, theme.caret, None, &rect);
                }
            }
            visible.push((line, y, has_caret));
        }

        if gutter_width > 0.0 {
            let rect = Rect::new(0.0, 0.0, gutter_width, self.size.height);
            scene.fill(
                Fill::NonZero,
                transform,
                theme.gutter_background,
                None,
                &rect,
            );
            let right = gutter_width - 2.0 * self.metrics.char_width;
            for &(line, y, has_caret) in &visible {
                let color = if has_caret {
                    theme.gutter_active_foreground
                } else {
                    theme.gutter_foreground
                };
                let layout = self.cache.number(cx, line + 1);
                let x = right - layout.full_width() as f64;
                draw_layout(
                    scene,
                    layout,
                    transform * Affine::translate(Vec2::new(x, y)),
                    Some(color),
                );
            }
        }

        scene.pop_layer();

        // Keep a screen's worth of layouts either side of the viewport for scrolling
        let last_visible = visible.last().map_or(first_visible, |v| v.0);
        let margin = last_visible - first_visible + 1;
        self.cache
            .evict_outside(first_visible.saturating_sub(margin)..last_visible + margin + 1);
    }
}

fn draw_layout(
    scene: &mut impl PaintScene,
    layout: &TextLayout,
    transform: Affine,
    brush_override: Option<Color>,
) {
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let run = glyph_run.run();
            let color = brush_override.unwrap_or(glyph_run.style().brush.0);
            let glyph_transform = run
                .synthesis()
                .skew()
                .map(|angle| Affine::skew(angle.to_radians().tan() as f64, 0.0));
            let normalized_coords: &[i16] = bytemuck::cast_slice(run.normalized_coords());
            scene.draw_glyphs(
                run.font(),
                run.font_size(),
                false,
                normalized_coords,
                Vec2::ZERO,
                Fill::NonZero,
                &anyrender::Paint::from(color),
                1.0,
                transform,
                glyph_transform,
                glyph_run.positioned_glyphs().map(|glyph| anyrender::Glyph {
                    id: glyph.id as _,
                    x: glyph.x,
                    y: glyph.y,
                }),
            );
        }
    }
}
