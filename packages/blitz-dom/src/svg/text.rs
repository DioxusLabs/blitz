//! `<text>` and `<tspan>` layout for native SVG.
//!
//! The text of a `<text>` element's subtree is shaped as one Parley line,
//! with a style span per element, then each typographic character is moved
//! to its SVG position (`x`/`y`/`dx`/`dy`/`rotate`), `textLength` is applied
//! and each text chunk is anchored (`text-anchor`).
//! <https://svgwg.org/svg2-draft/text.html#TextLayoutAlgorithm>

use std::collections::HashMap;
use std::ops::Range;

use blitz_traits::node_id::NodeId;
use kurbo::{Affine, Rect, Stroke};
use markup5ever::{local_name, ns};
use parley::{FontData, Layout, PositionedLayoutItem, TextStyle, TreeBuilder, WhiteSpaceCollapse};
use style::computed_values::direction::T as Direction;
use style::properties::ComputedValues;
use style::values::computed::{DominantBaseline, TextAnchor};
use style::values::specified::TextDecorationLine;

use super::{SvgViewport, attr, geometry};
use crate::NodeTree;
use crate::layout::text_transform::{CaseTransform, TextTransformer};
use crate::node::{ElementData, TextBrush};
use crate::stylo_to_parley;
use crate::{BaseDocument, CopyStyleGetters};

/// A laid out `<text>` element, in its user space.
#[derive(Debug, Clone, Default)]
pub struct SvgText {
    pub runs: Vec<SvgGlyphRun>,
    /// `text-decoration` lines, in paint order within their kind.
    pub decorations: Vec<SvgTextDecoration>,
    /// The union of the glyph cells (advance by ascent and descent).
    pub bbox: Option<Rect>,
    /// The cell of each typographic character, in paint order.
    pub cells: Vec<SvgTextCell>,
}

/// The cell of a typographic character (its advance by the font's ascent and
/// descent), which is its area for hit testing.
#[derive(Debug, Clone, Copy)]
pub struct SvgTextCell {
    /// The `<text>` or innermost `<tspan>` containing the character.
    pub node: NodeId,
    /// Maps `rect` onto the `<text>` element's user space.
    pub transform: Affine,
    pub rect: Rect,
}

/// Glyphs of one font, painted with the computed style of `node` (the
/// `<text>` or the innermost `<tspan>` containing them).
#[derive(Debug, Clone)]
pub struct SvgGlyphRun {
    pub node: NodeId,
    pub font: FontData,
    pub font_size: f32,
    pub normalized_coords: Vec<i16>,
    /// Synthetic oblique angle in degrees.
    pub skew: Option<f32>,
    /// The stroke style for `node`, when it has a visible stroke width.
    pub stroke: Option<Stroke>,
    pub glyphs: Vec<SvgGlyph>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvgGlyph {
    pub id: u32,
    /// The glyph origin in user space.
    pub x: f32,
    pub y: f32,
    /// Clockwise rotation about the origin in degrees (`rotate`).
    pub rotate: f32,
    /// Horizontal glyph scale (`lengthAdjust="spacingAndGlyphs"`).
    pub scale_x: f32,
}

impl SvgGlyph {
    /// The transform applied to the glyph outline about its origin.
    pub fn transform(&self) -> Option<Affine> {
        (self.rotate != 0.0 || self.scale_x != 1.0).then(|| {
            Affine::rotate((self.rotate as f64).to_radians())
                * Affine::scale_non_uniform(self.scale_x as f64, 1.0)
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgTextDecorationKind {
    Underline,
    Overline,
    LineThrough,
}

/// A decoration line, painted with the fill and stroke of `node`, the
/// element that specified it.
#[derive(Debug, Clone)]
pub struct SvgTextDecoration {
    pub node: NodeId,
    pub kind: SvgTextDecorationKind,
    pub rect: Rect,
    pub stroke: Option<Stroke>,
}

/// An element of the text subtree and the characters it contains.
struct TextElement {
    node: NodeId,
    chars: Range<usize>,
}

/// One typographic character (a Parley cluster), positioned in user space.
struct Item {
    /// Index of its first addressable character.
    char_index: usize,
    run: usize,
    /// The left edge of its advance on the baseline.
    x: f64,
    y: f64,
    advance: f64,
    rtl: bool,
    rotate: f64,
    scale_x: f64,
    /// Glyphs relative to the item's origin (unscaled).
    glyphs: Vec<(u32, f64, f64)>,
    ascent: f64,
    descent: f64,
    /// Underline offset and size, strikethrough offset and size, with
    /// offsets upwards from the baseline.
    decoration: [f64; 4],
    /// Where `x`/`y` was set explicitly, starting a new text chunk.
    absolute: bool,
}

impl Item {
    fn right(&self) -> f64 {
        self.x + self.advance
    }
}

/// Per-character values of the positioning attributes.
#[derive(Default, Clone, Copy)]
struct CharPosition {
    x: Option<f64>,
    y: Option<f64>,
    dx: f64,
    dy: f64,
    rotate: Option<f64>,
}

fn length_list(element: &ElementData, name: &str, basis: f64, font_size: f64) -> Vec<f64> {
    let Some(value) = attr(element, name) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for len in svgtypes::LengthListParser::from(value) {
        match len {
            Ok(len) => out.push(geometry::resolve_svg_length(len, basis, font_size)),
            // An invalid list is an error, which disables the attribute.
            Err(_) => return Vec::new(),
        }
    }
    out
}

fn number_list(element: &ElementData, name: &str) -> Vec<f64> {
    let Some(value) = attr(element, name) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for n in svgtypes::NumberListParser::from(value) {
        match n {
            Ok(n) => out.push(n),
            Err(_) => return Vec::new(),
        }
    }
    out
}

/// Pushes the addressable characters of a `<text>` subtree to a Parley
/// builder, applying `text-transform` and `white-space` collapsing across
/// element boundaries.
struct Collector<'a> {
    nodes: &'a NodeTree,
    /// Byte offset in the built text of each character.
    offsets: Vec<usize>,
    elements: Vec<TextElement>,
    transformer: TextTransformer,
    /// A collapsible space that is only kept if more text follows.
    pending_space: bool,
    /// Whether the last character can absorb a following collapsible space.
    after_space: bool,
}

impl Collector<'_> {
    fn collect(
        &mut self,
        builder: &mut TreeBuilder<'_, TextBrush>,
        id: NodeId,
        root: bool,
        parent_transform: &CaseTransform,
    ) {
        let node = &self.nodes[id];
        let Some(style) = node.svg_style() else {
            return;
        };
        if !root {
            builder.push_style_span(span_style(id, &style));
        }
        let case_transform = CaseTransform::from_style(&style);
        let case_transform = if style.get_inherited_text().text_transform.is_empty() {
            parent_transform
        } else {
            &case_transform
        };
        let preserve = style.get_inherited_text().white_space_collapse
            != style::computed_values::white_space_collapse::T::Collapse;
        let index = self.elements.len();
        let start = self.offsets.len();
        self.elements.push(TextElement {
            node: id,
            chars: start..start,
        });
        for &child_id in &node.children {
            let child = &self.nodes[child_id];
            if let Some(text) = child.text_data() {
                let content = self
                    .transformer
                    .transform(&text.content, case_transform, builder);
                let base = builder.text().len();
                let mut piece = String::with_capacity(content.len());
                let push = |piece: &mut String, offsets: &mut Vec<usize>, c: char| {
                    offsets.push(base + piece.len());
                    piece.push(c);
                };
                for c in content.chars() {
                    match c {
                        '\n' | '\r' | '\t' | ' ' if preserve => {
                            push(&mut piece, &mut self.offsets, ' ');
                            self.after_space = false;
                        }
                        '\n' | '\r' | '\t' | ' ' => {
                            if !self.after_space {
                                self.pending_space = true;
                            }
                        }
                        c => {
                            if std::mem::take(&mut self.pending_space) {
                                push(&mut piece, &mut self.offsets, ' ');
                            }
                            push(&mut piece, &mut self.offsets, c);
                            self.after_space = false;
                        }
                    }
                    if self.pending_space {
                        self.after_space = true;
                    }
                }
                if !piece.is_empty() {
                    builder.push_text(&piece);
                }
            } else if let Some(el) = child.element_data()
                && el.name.ns == ns!(svg)
                && matches!(el.name.local, local_name!("tspan") | local_name!("a"))
                && child
                    .svg_style()
                    .is_some_and(|s| !s.clone_display().is_none())
                && super::conditions_pass(el)
            {
                // A collapsed space before the element belongs to its parent.
                if self.pending_space && has_content(self.nodes, child_id) {
                    self.pending_space = false;
                    self.offsets.push(builder.text().len());
                    builder.push_text(" ");
                }
                self.collect(builder, child_id, false, case_transform);
            }
        }
        self.elements[index].chars.end = self.offsets.len();
        if !root {
            builder.pop_style_span();
        }
    }
}

/// Whether a subtree has any non-whitespace text.
fn has_content(nodes: &NodeTree, id: NodeId) -> bool {
    let node = &nodes[id];
    match node.text_data() {
        Some(text) => !text.content.trim_ascii().is_empty(),
        None => node.children.iter().any(|&child| has_content(nodes, child)),
    }
}

/// The Parley style for an element of the text subtree: shaped as one line.
fn span_style(id: NodeId, style: &ComputedValues) -> TextStyle<'static, 'static, TextBrush> {
    let mut span = stylo_to_parley::style(id, style);
    span.white_space_collapse = WhiteSpaceCollapse::Preserve;
    span.text_wrap_mode = parley::TextWrapMode::NoWrap;
    span
}

fn baseline_offset(style: &ComputedValues, ascent: f64, descent: f64, x_height: f64) -> f64 {
    match style.get_inherited_box().dominant_baseline {
        DominantBaseline::Auto | DominantBaseline::Alphabetic => 0.0,
        DominantBaseline::Middle => x_height / 2.0,
        DominantBaseline::Central => (ascent - descent) / 2.0,
        DominantBaseline::Hanging => ascent * 0.8,
        DominantBaseline::Mathematical => ascent * 0.5,
        DominantBaseline::TextTop => ascent,
        DominantBaseline::TextBottom | DominantBaseline::Ideographic => -descent,
    }
}

impl BaseDocument {
    /// Lay out a `<text>` element's content in its user space.
    pub(super) fn layout_svg_text(&mut self, text_id: NodeId, viewport: SvgViewport) -> SvgText {
        // Shape the text as one unwrapped line, in user units.
        let zoom_scale = viewport.unzoom(1.0) as f32;
        let mut layout: Layout<TextBrush> = Layout::new();
        let Some(root_style) = self.nodes[text_id].svg_style() else {
            return SvgText::default();
        };
        let rtl = root_style.clone_direction() == Direction::Rtl;
        let mut font_ctx = self.font_ctx.lock().unwrap();
        let mut builder = self.layout_ctx.tree_builder(
            &mut font_ctx,
            zoom_scale,
            true,
            &span_style(text_id, &root_style),
        );
        builder.set_base_direction(stylo_to_parley::base_direction(
            root_style.clone_direction(),
            root_style.clone_unicode_bidi(),
        ));
        let mut collector = Collector {
            nodes: &self.nodes,
            offsets: Vec::new(),
            elements: Vec::new(),
            transformer: TextTransformer::default(),
            pending_space: false,
            after_space: true,
        };
        collector.collect(&mut builder, text_id, true, &CaseTransform::NONE);
        let Collector {
            offsets, elements, ..
        } = collector;
        builder.build_into(&mut layout);
        drop(font_ctx);
        drop(root_style);
        if offsets.is_empty() {
            return SvgText::default();
        }
        let char_count = offsets.len();

        // Resolve the positioning attributes per character. Elements are in
        // tree order, so descendants override their ancestors.
        let mut positions = vec![CharPosition::default(); char_count];
        for el in &elements {
            let node = &self.nodes[el.node];
            let (Some(element), Some(style)) = (node.element_data(), node.svg_style()) else {
                continue;
            };
            let font_size = viewport.unzoom(geometry::font_size(&style));
            let chars = &mut positions[el.chars.clone()];
            let lists = [
                length_list(element, "x", viewport.width, font_size),
                length_list(element, "y", viewport.height, font_size),
                length_list(element, "dx", viewport.width, font_size),
                length_list(element, "dy", viewport.height, font_size),
            ];
            for (i, pos) in chars.iter_mut().enumerate() {
                if let Some(v) = lists[0].get(i) {
                    pos.x = Some(*v);
                }
                if let Some(v) = lists[1].get(i) {
                    pos.y = Some(*v);
                }
                if let Some(v) = lists[2].get(i) {
                    pos.dx = *v;
                }
                if let Some(v) = lists[3].get(i) {
                    pos.dy = *v;
                }
            }
            // The last rotation applies to the rest of the element's characters.
            let rotate = number_list(element, "rotate");
            if let Some(last) = rotate.last() {
                for (i, pos) in chars.iter_mut().enumerate() {
                    pos.rotate = Some(*rotate.get(i).unwrap_or(last));
                }
            }
        }

        layout.break_all_lines(None);

        let byte_to_char: HashMap<usize, usize> =
            offsets.iter().enumerate().map(|(i, b)| (*b, i)).collect();

        // Collect the shaped characters with their Parley positions.
        let mut runs = Vec::new();
        let mut items = Vec::new();
        for line in layout.lines() {
            let mut run_cursors = HashMap::new();
            let line_baseline = line.metrics().baseline as f64;
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                let run = glyph_run.run();
                let node = glyph_run.style().brush.id;
                let Some(style) = self.nodes[node].svg_style() else {
                    continue;
                };
                let metrics = run.font_metrics();
                let ascent = metrics.ascent as f64;
                let descent = metrics.descent as f64;
                let x_height = metrics.x_height.unwrap_or(metrics.ascent * 0.5) as f64;
                let y = glyph_run.baseline() as f64 - line_baseline
                    + baseline_offset(&style, ascent, descent, x_height);
                let decoration = [
                    metrics.underline_offset as f64,
                    metrics.underline_size as f64,
                    metrics.strikethrough_offset as f64,
                    metrics.strikethrough_size as f64,
                ];
                let run_index = runs.len();
                runs.push(SvgGlyphRun {
                    node,
                    font: run.font().clone(),
                    font_size: run.font_size(),
                    normalized_coords: run
                        .normalized_coords()
                        .iter()
                        .map(|c| c.to_bits())
                        .collect(),
                    skew: run.synthesis().skew(),
                    stroke: geometry::stroke_style(&style, viewport),
                    glyphs: Vec::new(),
                });
                // A shaping run is split into glyph runs by style, emitted in
                // visual order: take this glyph run's clusters from where the
                // previous one of the same run stopped.
                let style_index = glyph_run.style_index();
                let cursor = run_cursors.entry(run.index()).or_insert(0);
                let clusters: Vec<_> = run
                    .visual_clusters()
                    .skip(*cursor)
                    .skip_while(|c| c.style_index() != style_index)
                    .take_while(|c| c.style_index() == style_index)
                    .collect();
                let skipped = run
                    .visual_clusters()
                    .skip(*cursor)
                    .take_while(|c| c.style_index() != style_index)
                    .count();
                *cursor += skipped + clusters.len();
                let mut x = glyph_run.offset() as f64;
                for cluster in clusters {
                    let advance = cluster.advance() as f64;
                    let Some(&char_index) = byte_to_char.get(&cluster.text_range().start) else {
                        x += advance;
                        continue;
                    };
                    let mut pen = 0.0;
                    let glyphs = cluster
                        .glyphs()
                        .map(|g| {
                            let glyph = (g.id, pen + g.x as f64, g.y as f64);
                            pen += g.advance as f64;
                            glyph
                        })
                        .collect();
                    items.push(Item {
                        char_index,
                        run: run_index,
                        x,
                        y,
                        advance,
                        rtl: cluster.is_rtl(),
                        rotate: 0.0,
                        scale_x: 1.0,
                        glyphs,
                        ascent,
                        descent,
                        decoration,
                        absolute: false,
                    });
                    x += advance;
                }
            }
        }
        items.sort_by_key(|item| item.char_index);

        // https://svgwg.org/svg2-draft/text.html#TextLayoutAlgorithm
        // Adjust positions: dx, dy.
        let mut shift = (0.0, 0.0);
        for (i, item) in items.iter_mut().enumerate() {
            let pos = positions[item.char_index];
            item.absolute = i == 0 || pos.x.is_some() || pos.y.is_some();
            shift.0 += pos.dx;
            shift.1 += pos.dy;
            item.x += shift.0;
            item.y += shift.1;
            item.rotate = pos.rotate.unwrap_or(0.0);
        }

        // Apply `textLength`, innermost elements first.
        for el in elements.iter().rev() {
            let Some(element) = self.nodes[el.node].element_data() else {
                continue;
            };
            let Some(style) = self.nodes[el.node].svg_style() else {
                continue;
            };
            let font_size = viewport.unzoom(geometry::font_size(&style));
            let Some(text_length) = attr(element, "textLength")
                .and_then(|v| v.trim().parse::<svgtypes::Length>().ok())
                .map(|len| geometry::resolve_svg_length(len, viewport.width, font_size))
                .filter(|l| l.is_finite() && *l >= 0.0)
            else {
                continue;
            };
            let glyphs = attr(element, "lengthAdjust") == Some("spacingAndGlyphs");
            apply_text_length(&mut items, el.chars.clone(), text_length, glyphs);
        }

        // Adjust positions: x, y. The first character defaults to x=0, and
        // a character's own dx/dy still applies on top of its x/y.
        let mut shift = (0.0, 0.0);
        for (i, item) in items.iter_mut().enumerate() {
            let pos = positions[item.char_index];
            if let Some(x) = pos.x.or((i == 0).then_some(0.0)) {
                let origin = if item.rtl { item.right() } else { item.x };
                shift.0 = x + pos.dx - origin;
            }
            if let Some(y) = pos.y {
                shift.1 = y + pos.dy - item.y;
            }
            item.x += shift.0;
            item.y += shift.1;
        }

        // Anchor each text chunk.
        let mut start = 0;
        while start < items.len() {
            let end = items[start + 1..]
                .iter()
                .position(|item| item.absolute)
                .map_or(items.len(), |p| start + 1 + p);
            let chunk = &items[start..end];
            let node = elements
                .iter()
                .rev()
                .find(|el| el.chars.contains(&chunk[0].char_index))
                .map_or(text_id, |el| el.node);
            let anchor = self.nodes[node]
                .svg_style()
                .map_or(TextAnchor::Start, |s| s.get_inherited_svg().text_anchor);
            let min = chunk.iter().map(|i| i.x).fold(f64::INFINITY, f64::min);
            let max = chunk
                .iter()
                .map(Item::right)
                .fold(f64::NEG_INFINITY, f64::max);
            let first = &chunk[0];
            let origin = if first.rtl { first.right() } else { first.x };
            let delta = match (anchor, rtl) {
                (TextAnchor::Start, false) | (TextAnchor::End, true) => origin - min,
                (TextAnchor::Start, true) | (TextAnchor::End, false) => origin - max,
                (TextAnchor::Middle, _) => origin - (min + max) / 2.0,
            };
            for item in &mut items[start..end] {
                item.x += delta;
            }
            start = end;
        }

        let mut bbox: Option<Rect> = None;
        let mut cells = Vec::with_capacity(items.len());
        for item in &items {
            let rect = Rect::new(0.0, -item.ascent, item.advance, item.descent);
            let transform =
                Affine::translate((item.x, item.y)) * Affine::rotate(item.rotate.to_radians());
            let cell = transform.transform_rect_bbox(rect);
            bbox = Some(bbox.map_or(cell, |b| b.union(cell)));
            cells.push(SvgTextCell {
                node: runs[item.run].node,
                transform,
                rect,
            });
        }
        let decorations = self.svg_text_decorations(&elements, &items, viewport);
        for item in &items {
            let glyph_transform = Affine::rotate(item.rotate.to_radians());
            for &(id, gx, gy) in &item.glyphs {
                let offset = glyph_transform * kurbo::Point::new(gx * item.scale_x, gy);
                runs[item.run].glyphs.push(SvgGlyph {
                    id,
                    x: (item.x + offset.x) as f32,
                    y: (item.y + offset.y) as f32,
                    rotate: item.rotate as f32,
                    scale_x: item.scale_x as f32,
                });
            }
        }
        runs.retain(|run| !run.glyphs.is_empty());
        SvgText {
            runs,
            decorations,
            bbox,
            cells,
        }
    }

    fn svg_text_decorations(
        &self,
        elements: &[TextElement],
        items: &[Item],
        viewport: SvgViewport,
    ) -> Vec<SvgTextDecoration> {
        let mut out = Vec::new();
        for el in elements {
            let Some(style) = self.nodes[el.node].svg_style() else {
                continue;
            };
            let line = style.get_text().text_decoration_line;
            if line.is_empty() {
                continue;
            }
            let stroke = geometry::stroke_style(&style, viewport);
            for (flag, kind) in [
                (
                    TextDecorationLine::UNDERLINE,
                    SvgTextDecorationKind::Underline,
                ),
                (
                    TextDecorationLine::OVERLINE,
                    SvgTextDecorationKind::Overline,
                ),
                (
                    TextDecorationLine::LINE_THROUGH,
                    SvgTextDecorationKind::LineThrough,
                ),
            ] {
                if !line.contains(flag) {
                    continue;
                }
                let mut current: Option<Rect> = None;
                for item in items.iter().filter(|i| el.chars.contains(&i.char_index)) {
                    let [underline, size, strikethrough, strikethrough_size] = item.decoration;
                    let (offset, size) = match kind {
                        SvgTextDecorationKind::Underline => (underline, size),
                        SvgTextDecorationKind::Overline => (item.ascent, size),
                        SvgTextDecorationKind::LineThrough => (strikethrough, strikethrough_size),
                    };
                    let top = item.y - offset;
                    let rect = Rect::new(item.x, top, item.right(), top + size);
                    current = match current {
                        Some(r)
                            if (r.y0 - rect.y0).abs() < 1e-3 && (r.x1 - rect.x0).abs() < 1e-3 =>
                        {
                            Some(r.union(rect))
                        }
                        Some(r) => {
                            out.push(SvgTextDecoration {
                                node: el.node,
                                kind,
                                rect: r,
                                stroke: stroke.clone(),
                            });
                            Some(rect)
                        }
                        None => Some(rect),
                    };
                }
                if let Some(rect) = current {
                    out.push(SvgTextDecoration {
                        node: el.node,
                        kind,
                        rect,
                        stroke: stroke.clone(),
                    });
                }
            }
        }
        out
    }
}

/// Stretch or shrink the characters in `chars` to `text_length`.
/// <https://svgwg.org/svg2-draft/text.html#TextLayoutAlgorithm> (step 6)
fn apply_text_length(items: &mut [Item], chars: Range<usize>, text_length: f64, glyphs: bool) {
    let in_range: Vec<usize> = (0..items.len())
        .filter(|i| chars.contains(&items[*i].char_index))
        .collect();
    let Some(&last) = in_range.last() else {
        return;
    };
    let a = in_range
        .iter()
        .map(|i| items[*i].x)
        .fold(f64::INFINITY, f64::min);
    let b = in_range
        .iter()
        .map(|i| items[*i].right())
        .fold(f64::NEG_INFINITY, f64::max);
    let length = b - a;
    let delta = text_length - length;
    if glyphs {
        if length <= 0.0 {
            return;
        }
        let scale = text_length / length;
        for &i in &in_range {
            let item = &mut items[i];
            item.x = a + (item.x - a) * scale;
            item.advance *= scale;
            item.scale_x *= scale;
        }
    } else {
        let n = in_range.len();
        if n < 2 {
            return;
        }
        let step = delta / (n - 1) as f64;
        for (k, &i) in in_range.iter().enumerate() {
            items[i].x += step * k as f64;
        }
    }
    // Later characters move with the end of the range until the next
    // absolutely positioned one.
    for item in items[last + 1..].iter_mut() {
        if item.absolute {
            break;
        }
        item.x += delta;
    }
}
