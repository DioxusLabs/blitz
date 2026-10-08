//! Painting text laid out by winkin.
//!
//! winkin hands out geometry and order, and nothing it hands out is drawn by
//! it: each line's [`Line::paints`] walks what the line paints in Chrome's
//! order, and this draws each with what the node it names is styled in.

use core::cell::Cell;

use anyrender::{Glyph, PaintScene};
use blitz_dom::node::Marker;
use blitz_dom::text::EditableText as _;
use blitz_dom::text::winkin::winkin;
use blitz_dom::text::winkin::{
    MarkerLayout, TextEditor, TextLayout, line_frame, page, relative_shift,
};
use blitz_dom::text::InlineText as _;
use blitz_dom::{BaseDocument, NodeId};
use kurbo::{Affine, Insets, Point, Rect, Vec2};
use peniko::{Blob, Fill, FontData};
use style::properties::ComputedValues;
use style::servo_arc::Arc as ServoArc;
use style::values::computed::{BorderCornerRadius, CSSPixelLength, TextDecorationLine};
use winkin::paint::{Decorates, Decoration, EmphasisMark, Paint};
use winkin::style::WritingMode;
use winkin::{BoxFragment, FontInstance, Layout, NodeKey, RunOrientation, TextRun};

use crate::color::{Color, ToColorColor as _};
use crate::kurbo_css::{CssBox, Edge, NonUniformRoundedRectRadii};
use super::{DecorationRunGeometry, DrawTextContext, LineDecoration};

/// What the winkin painter keeps between uses: nothing beyond the shared decoration buffers.
#[derive(Default)]
pub(super) struct Scratch;

/// A box's four sides in a line's own terms, from its physical ones: its
/// line-left and line-right, and its over and under sides.
#[derive(Copy, Clone)]
struct LineSides<T> {
    left: T,
    right: T,
    over: T,
    under: T,
}

impl<T> LineSides<T> {
    /// `top`, `right`, `bottom` and `left` where a line in `writing_mode`
    /// puts them.
    fn from_physical(writing_mode: WritingMode, top: T, right: T, bottom: T, left: T) -> Self {
        match writing_mode {
            WritingMode::HorizontalTb => Self {
                left,
                right,
                over: top,
                under: bottom,
            },
            WritingMode::VerticalRl | WritingMode::VerticalLr | WritingMode::SidewaysRl => Self {
                left: top,
                right: bottom,
                over: right,
                under: left,
            },
            WritingMode::SidewaysLr => Self {
                left: bottom,
                right: top,
                over: left,
                under: right,
            },
        }
    }
}

/// Paints every line of `layout`: the boxes, decorations, ruby annotations,
/// text, emphasis marks and generated text of each, in Chrome's order.
///
/// `transform` takes the content box's device pixels onto the scene, and
/// `root` is the block the layout was built for.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint(
    scene: &mut impl PaintScene,
    text: &TextLayout,
    layout: &Layout,
    doc: &BaseDocument,
    transform: Affine,
    content_width: f64,
    content_height: f64,
    scale: f64,
    root: NodeId,
    context: &mut DrawTextContext,
    selection: Option<(usize, usize)>,
) {
    let writing_mode = text.writing_mode();
    let page = page(writing_mode, content_width, content_height);
    let selection_rects: Vec<_> = selection
        .map(|(start, end)| layout.selection_rects(start..end).collect())
        .unwrap_or_default();
    let root_key = root.as_u64();
    let root_styles = doc
        .get_node(root)
        .and_then(|node| node.primary_styles())
        .map(|styles| (*styles).clone());
    // Whether glyphs stand upright on the line's central baseline, which
    // decorations are measured about.
    let centered = matches!(
        writing_mode,
        WritingMode::VerticalRl | WritingMode::VerticalLr
    ) && root_styles.as_ref().is_none_or(|styles| {
        styles.clone_text_orientation() != style::computed_values::text_orientation::T::Sideways
    });
    let rtl = root_styles.as_ref().is_some_and(|styles| {
        styles.clone_direction() == style::computed_values::direction::T::Rtl
    });
    let propagated = propagated_decorations(doc, root);
    let painter = Painter {
        text,
        doc,
        transform,
        containing: kurbo::Size::new(content_width / scale, content_height / scale),
        rtl,
        shifted: Cell::new(None),
        scale,
        writing_mode,
        root_key,
        root_styles,
        propagated,
        centered,
        color: Cell::new(None),
        line: Cell::new((Affine::IDENTITY, 0.0)),
    };
    let mut items = Vec::new();
    for line in layout.lines() {
        // The block's first formatted line is set in its `::first-line`
        // styles, where it has them.
        let first = line.index() == 0;
        let metrics = line.metrics();
        let frame = line_frame(writing_mode, page, &metrics);
        painter.line.set((frame, f64::from(metrics.height())));
        items.clear();
        items.extend(line.paints(|key| painter.decorates(key, first)));
        // Text shadows go under everything the line paints, the last of each
        // list lowest, then the line is painted over them.
        let shadows = items
            .iter()
            .filter_map(|item| painter.shadows_of(item, first))
            .max()
            .unwrap_or(0);
        for pass in (0..shadows).rev().map(Some).chain([None]) {
            // The run whose marks follow it, and whose font they are set in.
            let mut marked: Option<TextRun<'_>> = None;
            let mut selection_drawn = false;
            for item in &items {
                // Backgrounds precede the highlight; glyphs follow it.
                if pass.is_none()
                    && !selection_drawn
                    && !matches!(item, Paint::Background(_) | Paint::Box(_))
                {
                    paint_selection(scene, &selection_rects, line.index(), frame, transform);
                    selection_drawn = true;
                }
                match *item {
                    Paint::Background(background) if pass.is_none() => {
                        painter.paint_first_line_background(scene, frame, first, background);
                    }
                    Paint::Box(fragment) if pass.is_none() => {
                        painter.paint_box(scene, frame, &fragment, first);
                    }
                    Paint::DecorationBeforeText(bar) => {
                        painter.paint_decoration(scene, frame, &bar, first, false, pass, context);
                    }
                    Paint::DecorationAfterText(bar) => {
                        painter.paint_decoration(scene, frame, &bar, first, true, pass, context);
                    }
                    Paint::Annotation(run) | Paint::Text(run) => {
                        painter.draw_run(scene, frame, &run, first, pass);
                        marked = Some(run);
                    }
                    Paint::Generated(run) => painter.draw_run(scene, frame, &run, first, pass),
                    Paint::Emphasis(mark) => {
                        painter.draw_emphasis(scene, frame, &mark, marked.as_ref(), first, pass);
                    }
                    // Anything else winkin paints draws nothing here: atomic
                    // inlines are painted as boxes of their own.
                    _ => {}
                }
            }
            if pass.is_none() && !selection_drawn {
                paint_selection(scene, &selection_rects, line.index(), frame, transform);
            }
        }
    }
}

fn paint_selection(
    scene: &mut impl PaintScene,
    rects: &[winkin::selection::SelectionRect],
    line: usize,
    frame: Affine,
    transform: Affine,
) {
    for rect in rects.iter().filter(|rect| rect.line == line) {
        let shape = Rect::new(
            f64::from(rect.inline.left),
            f64::from(rect.block.over),
            f64::from(rect.inline.right),
            f64::from(rect.block.under),
        );
        scene.fill(
            Fill::NonZero,
            transform * frame,
            crate::SELECTION_COLOR,
            None,
            &shape,
        );
    }
}

/// What one layout is painted with.
struct Painter<'a> {
    text: &'a TextLayout,
    doc: &'a BaseDocument,
    transform: Affine,
    /// The block's content box in CSS pixels, which relative offsets'
    /// percentages are of.
    containing: kurbo::Size,
    /// Whether the block is right-to-left, where `right` outweighs `left`.
    rtl: bool,
    /// The scene transform `position: relative` moved last, and whose it was.
    shifted: Cell<Option<(u64, Affine)>>,
    scale: f64,
    writing_mode: WritingMode,
    root_key: u64,
    root_styles: Option<ServoArc<ComputedValues>>,
    /// The styles of the boxes around the block whose decorations it
    /// draws, the outermost first.
    propagated: Vec<ServoArc<ComputedValues>>,
    centered: bool,
    /// The text color painted last, and whose it was.
    color: Cell<Option<(u64, bool, Color)>>,
    /// The frame of the line being painted, and its height: what a blurred
    /// shadow is drawn within is found from them.
    line: Cell<(Affine, f64)>,
}

impl Painter<'_> {
    /// What takes the content box's device pixels onto the scene for what
    /// `key` paints: moved as `position: relative` moves its inline boxes.
    fn transform(&self, key: u64) -> Affine {
        if key == self.root_key {
            return self.transform;
        }
        if let Some((held, transform)) = self.shifted.get()
            && held == key
        {
            return transform;
        }
        let shift = relative_shift(self.doc, key, self.containing, self.rtl);
        let transform = if shift == Vec2::ZERO {
            self.transform
        } else {
            self.transform * Affine::translate(shift * self.scale)
        };
        self.shifted.set(Some((key, transform)));
        transform
    }

    /// The computed style `key` is painted in, on the first line or not.
    fn styles(&self, key: u64, first: bool) -> Option<ServoArc<ComputedValues>> {
        self.text.computed_style(self.doc, key, first)
    }

    /// The color of `key`'s text, on the first line or not.
    fn color(&self, key: u64, first: bool) -> Color {
        if let Some((held, held_first, color)) = self.color.get()
            && held == key
            && held_first == first
        {
            return color;
        }
        let color = self
            .styles(key, first)
            .map_or(Color::BLACK, |styles| styles.clone_color().as_srgb_color());
        self.color.set(Some((key, first, color)));
        color
    }

    /// The styles whose decorations `key` draws as a decorating box: its
    /// own, and for the block, first those its ancestors propagate to it,
    /// and on the first line its `::first-line` as well, whose decorations
    /// do not inherit into the block's.
    fn decorating_styles(
        &self,
        key: u64,
        first: bool,
    ) -> impl Iterator<Item = ServoArc<ComputedValues>> + '_ {
        let (propagated, own, first_line) = if key == self.root_key {
            let first_line = first.then(|| self.text.first_line_style_of(key)).flatten();
            (&self.propagated[..], self.root_styles.clone(), first_line)
        } else {
            (&[][..], self.styles(key, first), None)
        };
        propagated.iter().cloned().chain(own).chain(first_line)
    }

    /// Which decoration lines `key` draws: the question [`Line::paints`]
    /// asks of the block and each box.
    fn decorates(&self, key: NodeKey, first: bool) -> Decorates {
        let mut before = false;
        let mut after = false;
        for styles in self.decorating_styles(key.0, first) {
            // A `display: contents` element is no box, and decorates nothing.
            if styles.clone_display().is_contents() {
                continue;
            }
            let lines = styles.clone_text_decoration_line();
            before |=
                lines.intersects(TextDecorationLine::UNDERLINE | TextDecorationLine::OVERLINE);
            after |= lines.contains(TextDecorationLine::LINE_THROUGH);
        }
        match (before, after) {
            (true, true) => Decorates::Both,
            (true, false) => Decorates::BeforeText,
            (false, true) => Decorates::AfterText,
            (false, false) => Decorates::None,
        }
    }

    /// Paints the block's `::first-line` background behind the first line's
    /// content, where it sets one. The block's own background is its box's,
    /// which Blitz paints with the box.
    fn paint_first_line_background(
        &self,
        scene: &mut impl PaintScene,
        frame: Affine,
        first: bool,
        background: winkin::paint::Background,
    ) {
        if !first {
            return;
        }
        let Some(styles) = self.text.first_line_style_of(self.root_key) else {
            return;
        };
        let color = styles
            .get_background()
            .background_color
            .resolve_to_absolute(&styles.clone_color())
            .as_srgb_color();
        if color.components[3] <= 0.0 {
            return;
        }
        let (inline, block) = (background.inline(), background.block());
        let rect = Rect::new(
            f64::from(inline.left),
            f64::from(block.over),
            f64::from(inline.right),
            f64::from(block.under),
        );
        scene.fill(Fill::NonZero, self.transform * frame, color, None, &rect);
    }

    /// Draws a stretch of an inline box: its background and its borders,
    /// and no border on a side where the box goes on past it.
    fn paint_box(
        &self,
        scene: &mut impl PaintScene,
        frame: Affine,
        fragment: &BoxFragment<'_>,
        first: bool,
    ) {
        let key = fragment.key().0;
        let Some(styles) = self.styles(key, first) else {
            return;
        };
        let current = styles.clone_color();
        let background = styles
            .get_background()
            .background_color
            .resolve_to_absolute(&current)
            .as_srgb_color();
        let border = styles.get_border();
        let scale = self.scale;
        // Snapped as the layout took them: a width under a pixel to one, and
        // any other down to a whole one.
        let width = |width: &style::values::computed::BorderSideWidth,
                     style: style::values::specified::border::BorderStyle| {
            let width = f64::from(width.0.to_f32_px()) * scale;
            if style.none_or_hidden() || width <= 0.0 {
                0.0
            } else {
                width.floor().max(1.0)
            }
        };
        let widths = LineSides::from_physical(
            self.writing_mode,
            width(&border.border_top_width, border.border_top_style),
            width(&border.border_right_width, border.border_right_style),
            width(&border.border_bottom_width, border.border_bottom_style),
            width(&border.border_left_width, border.border_left_style),
        );
        let colors = LineSides::from_physical(
            self.writing_mode,
            &border.border_top_color,
            &border.border_right_color,
            &border.border_bottom_color,
            &border.border_left_color,
        );
        let (inline, block) = (fragment.inline(), fragment.block());
        let border_box = Rect::new(
            f64::from(inline.left),
            f64::from(block.over),
            f64::from(inline.right),
            f64::from(block.under),
        );
        if border_box.width() <= 0.0 && border_box.height() <= 0.0 {
            return;
        }
        let open_left = fragment.is_open_left();
        let open_right = fragment.is_open_right();
        let insets = Insets {
            x0: if open_left { 0.0 } else { widths.left },
            y0: widths.over,
            x1: if open_right { 0.0 } else { widths.right },
            y1: widths.under,
        };
        // Each corner of the stretch is a corner of the page's box, which one
        // being the writing mode's; in a vertical line a corner's horizontal
        // radius runs across the line.
        let [left_over, right_over, right_under, left_under] =
            line_corners(self.writing_mode, border);
        let across_is_horizontal = self.writing_mode != WritingMode::HorizontalTb;
        let radius = |corner: &BorderCornerRadius, open: bool| {
            if open {
                return Vec2::ZERO;
            }
            let resolve = |length: &style::values::computed::LengthPercentage, of: f64| {
                scale
                    * f64::from(
                        length
                            .resolve(CSSPixelLength::new((of / scale) as f32))
                            .px(),
                    )
            };
            let (physical_width, physical_height) = if across_is_horizontal {
                (border_box.height(), border_box.width())
            } else {
                (border_box.width(), border_box.height())
            };
            let horizontal = resolve(&corner.0.width.0, physical_width);
            let vertical = resolve(&corner.0.height.0, physical_height);
            if across_is_horizontal {
                Vec2::new(vertical, horizontal)
            } else {
                Vec2::new(horizontal, vertical)
            }
        };
        let radii = NonUniformRoundedRectRadii {
            top_left: radius(left_over, open_left),
            top_right: radius(right_over, open_right),
            bottom_right: radius(right_under, open_right),
            bottom_left: radius(left_under, open_left),
        };
        let css_box = CssBox::new(border_box, insets, Insets::ZERO, 0.0, radii);
        let transform = self.transform(key) * frame;
        if background.components[3] > 0.0 {
            scene.fill(
                Fill::NonZero,
                transform,
                background,
                None,
                &css_box.border_box_path(),
            );
        }
        for (edge, width, color) in [
            (Edge::Top, insets.y0, colors.over),
            (Edge::Right, insets.x1, colors.right),
            (Edge::Bottom, insets.y1, colors.under),
            (Edge::Left, insets.x0, colors.left),
        ] {
            if width > 0.0 {
                let color = color.resolve_to_absolute(&current).as_srgb_color();
                let shape = css_box.border_edge_shape(edge);
                scene.fill(Fill::NonZero, transform, color, None, &shape);
            }
        }
    }

    /// Draws one bar of a decoration, with the decoration `bar`'s box sets.
    fn paint_decoration(
        &self,
        scene: &mut impl PaintScene,
        frame: Affine,
        bar: &Decoration,
        first: bool,
        after_text: bool,
        pass: Option<usize>,
        context: &mut DrawTextContext,
    ) {
        let inline = bar.inline();
        let key = bar.key().0;
        let geometry = WinkinDecoration {
            left: f64::from(inline.left),
            right: f64::from(inline.right),
            baseline: bar.baseline(),
            ascent: bar.ascent(),
            descent: bar.descent(),
            underline_gap: bar.underline_gap(),
            thickness: bar.underline_thickness(),
            font_size: bar.font_size(),
            centered: self.centered,
        };
        self.shadowed(
            scene,
            pass,
            self.shadow_styles(key, first).as_deref(),
            self.color(key, first),
            self.transform(key),
            (geometry.left, geometry.right),
            None,
            |scene, placed, color| {
                for styles in self.decorating_styles(key, first) {
                    context.draw_winkin_decoration(
                        scene,
                        placed * frame,
                        self.scale,
                        &styles,
                        geometry,
                        after_text,
                        color,
                    );
                }
            },
        );
    }

    /// The style whose text shadows what `key` paints casts.
    fn shadow_styles(&self, key: u64, first: bool) -> Option<ServoArc<ComputedValues>> {
        self.styles(key, first)
    }

    /// How many text shadows `item` casts, where it casts any.
    fn shadows_of(&self, item: &Paint<'_>, first: bool) -> Option<usize> {
        let key = match item {
            Paint::DecorationBeforeText(bar) | Paint::DecorationAfterText(bar) => bar.key(),
            Paint::Annotation(run) | Paint::Text(run) | Paint::Generated(run) => run.key(),
            Paint::Emphasis(mark) => mark.key,
            _ => return None,
        };
        let styles = self.shadow_styles(key.0, first)?;
        Some(styles.get_inherited_text().text_shadow.0.len())
    }

    /// Draws what `draw` paints: in the foreground pass, where `pass` is
    /// `None`, in `own`; in a shadow pass, as the text shadow of that index
    /// in `styles` casts it, moved by its offset, in its color and blurred
    /// by its radius, where `styles` has that many.
    ///
    /// `draw` is handed the transform taking the content box's device pixels
    /// onto the scene, `placed` moved by the shadow, and the color to paint
    /// in where it is not its own; `current` is what `currentcolor` is.
    /// `inline` is where along the line what it paints starts and ends,
    /// which a blurred shadow is drawn around.
    #[allow(clippy::too_many_arguments)]
    fn shadowed<S: PaintScene>(
        &self,
        scene: &mut S,
        pass: Option<usize>,
        styles: Option<&ComputedValues>,
        current: Color,
        placed: Affine,
        inline: (f64, f64),
        own: Option<Color>,
        mut draw: impl FnMut(&mut S, Affine, Option<Color>),
    ) {
        let Some(index) = pass else {
            draw(scene, placed, own);
            return;
        };
        if let Some(styles) = styles
            && let Some(shadow) = styles.get_inherited_text().text_shadow.0.get(index)
        {
            {
                let color = if shadow.color.is_currentcolor() {
                    current
                } else {
                    shadow
                        .color
                        .resolve_to_absolute(&styles.clone_color())
                        .as_srgb_color()
                };
                if color.components[3] <= 0.0 {
                    return;
                }
                let offset = Vec2::new(
                    f64::from(shadow.horizontal.px()),
                    f64::from(shadow.vertical.px()),
                ) * self.scale;
                let moved = placed * Affine::translate(offset);
                // A blur radius is twice the deviation of its Gaussian.
                let deviation = f64::from(shadow.blur.0.px()) * self.scale / 2.0;
                if deviation > 0.0 {
                    // Glyphs reach past the line box by up to about its
                    // height, and the blur past them.
                    let (frame, height) = self.line.get();
                    let reach = 3.0 * deviation + height;
                    let clip = frame
                        .transform_rect_bbox(Rect::new(inline.0, 0.0, inline.1, height))
                        .inflate(reach, reach);
                    let filter = anyrender::filters::Filter::single(
                        anyrender::filters::FilterEffect::blur(deviation as f32),
                    );
                    scene.push_layer(
                        Fill::NonZero,
                        peniko::Mix::Normal,
                        1.0,
                        moved,
                        &clip,
                        Some(std::sync::Arc::new(filter)),
                        None,
                    );
                    draw(scene, moved, Some(color));
                    scene.pop_layer();
                } else {
                    draw(scene, moved, Some(color));
                }
            }
        }
    }

    /// Draws the glyphs of a run of text, in its node's color.
    ///
    /// A glyph standing upright in a vertical line, or combined into one em,
    /// is not turned with the line: only the point it is drawn at is.
    fn draw_run(
        &self,
        scene: &mut impl PaintScene,
        frame: Affine,
        run: &TextRun<'_>,
        first: bool,
        pass: Option<usize>,
    ) {
        let Some(font) = run.font() else {
            return;
        };
        let key = run.key().0;
        let color = self.color(key, first);
        let upright = matches!(
            run.orientation(),
            RunOrientation::Upright | RunOrientation::Combined
        );
        // Combined text squeezed into its em: its places already are, and
        // each glyph is drawn that much narrower about its own origin.
        let squeeze = run.combine_scale();
        let mut glyph_transform =
            (squeeze < 1.0).then(|| Affine::scale_non_uniform(f64::from(squeeze), 1.0));
        if let Some(skew) = font.skew {
            // CSS's clockwise degrees lean the glyph's top to the right.
            let lean = Affine::skew(-f64::from(skew.to_radians().tan()), 0.0);
            glyph_transform = Some(glyph_transform.map_or(lean, |squeeze| squeeze * lean));
        }
        // The renderer walks the glyphs more than once, so the walk is cloned.
        let glyphs = || {
            run.glyphs().map(move |glyph| {
                if upright {
                    let at = frame * Point::new(f64::from(glyph.x), f64::from(glyph.y));
                    Glyph {
                        id: glyph.id,
                        x: at.x as f32,
                        y: at.y as f32,
                    }
                } else {
                    Glyph {
                        id: glyph.id,
                        x: glyph.x,
                        y: glyph.y,
                    }
                }
            })
        };
        let data = font_data(&font);
        let inline = if pass.is_some() {
            run.glyphs()
                .fold((f64::MAX, f64::MIN), |(start, end), glyph| {
                    let x = f64::from(glyph.x);
                    (start.min(x), end.max(x))
                })
        } else {
            (0.0, 0.0)
        };
        self.shadowed(
            scene,
            pass,
            self.styles(key, first).as_deref(),
            color,
            self.transform(key),
            inline,
            Some(color),
            |scene, placed, color| {
                scene.draw_glyphs(
                    &data,
                    font.size,
                    true,
                    font.coords,
                    embolden(&font),
                    Fill::NonZero,
                    &anyrender::Paint::from(color.unwrap_or(Color::BLACK)),
                    1.0,
                    if upright { placed } else { placed * frame },
                    glyph_transform,
                    glyphs(),
                );
            },
        );
    }

    /// Draws an emphasis mark where winkin put it: the mark string laid out
    /// in the fonts of the text it marks, at the marks' size, its advance
    /// centred on the mark's middle and standing on its baseline.
    ///
    /// Upright in a vertical line, the mark is drawn unturned, its em box
    /// centred on the point winkin gives.
    fn draw_emphasis(
        &self,
        scene: &mut impl PaintScene,
        frame: Affine,
        mark: &EmphasisMark,
        run: Option<&TextRun<'_>>,
        first: bool,
        pass: Option<usize>,
    ) {
        let key = mark.key.0;
        let Some(styles) = self.styles(key, first) else {
            return;
        };
        // The marks are laid out per element: a text node's is its parent's.
        let element = self
            .doc
            .get_node(NodeId::from_u64(key))
            .and_then(|node| match node.data {
                blitz_dom::NodeData::Text(_) => node.parent,
                _ => Some(node.id),
            });
        let Some(layout) = element.and_then(|element| self.text.emphasis_mark(element.as_u64()))
        else {
            return;
        };
        let Some(line) = layout.lines().next() else {
            return;
        };
        let metrics = line.metrics();
        // Laid out at the size of the marks off the first line, which may
        // differ on it.
        let built = line
            .paints(|_| Decorates::None)
            .find_map(|item| match item {
                Paint::Text(run) => run.font().map(|font| font.size),
                _ => None,
            })
            .unwrap_or(mark.size);
        let ratio = if built > 0.0 { mark.size / built } else { 1.0 };
        // `currentcolor` is the text's own, which on the first line may be
        // its `::first-line` color.
        let text_color = self.color(key, first);
        let emphasis_color = &styles.get_inherited_text().text_emphasis_color;
        let color = if emphasis_color.is_currentcolor() {
            text_color
        } else {
            emphasis_color
                .resolve_to_absolute(&styles.clone_color())
                .as_srgb_color()
        };
        let upright = run.is_some_and(|run| {
            matches!(
                run.orientation(),
                RunOrientation::Upright | RunOrientation::Combined
            )
        });
        let baseline = metrics.baseline - metrics.top;
        let half = metrics.width / 2.0;
        // Where the mark's own line's origin goes, and what it is drawn
        // with: along the line's frame, or unturned about the point.
        let (origin, frame_after) = if upright {
            let center = frame * Point::new(f64::from(mark.x), f64::from(mark.baseline));
            let middle = f64::from((metrics.ascent - metrics.descent) / 2.0 * ratio);
            (Point::new(center.x, center.y + middle), Affine::IDENTITY)
        } else {
            (
                Point::new(f64::from(mark.x), f64::from(mark.baseline)),
                frame,
            )
        };
        let place = move |x: f32, y: f32| Glyph {
            id: 0,
            x: (origin.x + f64::from((x - half) * ratio)) as f32,
            y: (origin.y + f64::from((y - baseline) * ratio)) as f32,
        };
        self.shadowed(
            scene,
            pass,
            Some(&styles),
            text_color,
            self.transform(key),
            (f64::from(mark.x - mark.size), f64::from(mark.x + mark.size)),
            Some(color),
            |scene, placed, color| {
                for item in line.paints(|_| Decorates::None) {
                    let Paint::Text(mark_run) = item else {
                        continue;
                    };
                    let Some(font) = mark_run.font() else {
                        continue;
                    };
                    scene.draw_glyphs(
                        &font_data(&font),
                        font.size * ratio,
                        true,
                        font.coords,
                        embolden(&font),
                        Fill::NonZero,
                        &anyrender::Paint::from(color.unwrap_or(Color::BLACK)),
                        1.0,
                        placed * frame_after,
                        None,
                        mark_run.glyphs().map(|glyph| Glyph {
                            id: glyph.id,
                            ..place(glyph.x, glyph.y)
                        }),
                    );
                }
            },
        );
    }
}

/// The styles of the boxes around `root` whose decorations propagate to its
/// inline content, the outermost first.
///
/// Decorations propagate to in-flow children, so the walk stops at a box
/// that is floated, absolutely positioned or an atomic inline: its own
/// decorations are its content's, its ancestors' are not.
fn propagated_decorations(doc: &BaseDocument, root: NodeId) -> Vec<ServoArc<ComputedValues>> {
    use style::values::specified::box_::{DisplayInside, DisplayOutside};
    let mut propagated = Vec::new();
    let mut at = doc.get_node(root);
    while let Some(node) = at {
        let Some(styles) = node.primary_styles() else {
            // An anonymous block may have no style of its own: it is in the
            // flow of the box around it.
            at = node.parent.and_then(|parent| doc.get_node(parent));
            continue;
        };
        if node.id != root && !styles.clone_text_decoration_line().is_empty() {
            propagated.push((*styles).clone());
        }
        let display = styles.clone_display();
        let atomic = display.outside() == DisplayOutside::Inline
            && display.inside() != DisplayInside::Flow
            && !display.is_contents();
        if atomic
            || styles.clone_position().is_absolutely_positioned()
            || styles.clone_float() != style::values::computed::Float::None
        {
            break;
        }
        at = node.parent.and_then(|parent| doc.get_node(parent));
    }
    propagated.reverse();
    propagated
}

/// A box's corners in a line's own terms: at its line-left end on its over
/// side, at its line-right end on its over side, then under it at its
/// line-right and line-left ends.
fn line_corners(
    writing_mode: WritingMode,
    border: &style::properties::style_structs::Border,
) -> [&BorderCornerRadius; 4] {
    let (top_left, top_right, bottom_right, bottom_left) = (
        &border.border_top_left_radius,
        &border.border_top_right_radius,
        &border.border_bottom_right_radius,
        &border.border_bottom_left_radius,
    );
    match writing_mode {
        WritingMode::HorizontalTb => [top_left, top_right, bottom_right, bottom_left],
        // Lines down the page, over on their right.
        WritingMode::VerticalRl | WritingMode::VerticalLr | WritingMode::SidewaysRl => {
            [top_right, bottom_right, bottom_left, top_left]
        }
        // Lines up the page, over on their left.
        WritingMode::SidewaysLr => [bottom_left, top_left, top_right, bottom_right],
    }
}

/// A run's font as the renderer draws it, sharing its bytes and their id so
/// the renderer's caches key it as fontwich does.
fn font_data(font: &FontInstance<'_>) -> FontData {
    FontData::new(
        Blob::from_raw_parts(font.bytes.arc().clone(), font.bytes.id()),
        font.index,
    )
}

/// How far a glyph's outline is grown where its family has no bold and it
/// is emboldened: Skia's fake bold, a 24th of the size at 9 pixels to a 32nd
/// at 36, half each side.
fn embolden(font: &FontInstance<'_>) -> Vec2 {
    if !font.embolden {
        return Vec2::ZERO;
    }
    let size = f64::from(font.size);
    let ratio = {
        let t = ((size - 9.0) / 27.0).clamp(0.0, 1.0);
        (1.0 / 24.0) * (1.0 - t) + (1.0 / 32.0) * t
    };
    let extra = size * ratio / 2.0;
    Vec2::new(extra, extra)
}

/// Paints an inline formatting context laid out by winkin.
///
/// `transform` takes the content box's device pixels, moved down by the layout's block offset,
/// onto the scene, and `content_size` is the content box's size in device pixels.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_inline_layout(
    scene: &mut impl PaintScene,
    text_layout: &TextLayout,
    doc: &BaseDocument,
    transform: Affine,
    content_size: kurbo::Size,
    scale: f64,
    root: NodeId,
    selection: Option<(usize, usize)>,
    context: &mut DrawTextContext,
) {
    let Some(layout) = text_layout.layout() else {
        return;
    };
    paint(
        scene,
        text_layout,
        layout,
        doc,
        transform,
        content_size.width,
        content_size.height,
        scale,
        root,
        context,
        selection,
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
    let text_layout = editor.text_layout();
    let Some(layout) = text_layout.layout() else {
        return;
    };
    let size = editor.metrics().map(|metrics| metrics.size).unwrap_or_default();
    let selection = editor.selection_range();
    let selection = (focussed && !selection.is_empty()).then_some((selection.start, selection.end));
    paint(
        scene,
        text_layout,
        layout,
        doc,
        transform,
        size.width,
        size.height,
        scale,
        node_id,
        context,
        selection,
    );
    if focussed
        && editor.is_selection_collapsed()
        && let Some(caret) = editor.caret_rect()
    {
        scene.fill(Fill::NonZero, transform, caret_color, None, &caret);
    }
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
    marker_layout: &MarkerLayout,
    text_layout: Option<&TextLayout>,
    item_layout: &taffy::Layout,
    doc: &BaseDocument,
    pos: Point,
    transform: Affine,
    scale: f64,
    node_id: NodeId,
    context: &mut DrawTextContext,
) {
    let Some(layout) = marker_layout.layout() else {
        return;
    };
    let Some(line) = layout.lines().next() else {
        return;
    };
    let metrics = line.metrics();
    let marker_scale = marker_layout.scale();
    // Right align and pad the bullet when rendering outside
    let x_padding = match marker {
        Marker::Char(_) => 8.0,
        Marker::String(_) => 0.0,
    };
    // Outside markers are placed outside the list item's border box
    // (`pos` is the origin of its content box)
    let x_offset = -((metrics.left + metrics.width) / marker_scale
        + x_padding
        + item_layout.padding.left
        + item_layout.border.left);
    // Align the marker with the baseline of the first line of text in the list item
    let y_offset = text_layout
        .and_then(|text_layout| {
            let first = text_layout.layout()?.lines().next()?.metrics();
            Some((first.baseline - metrics.baseline) / marker_scale + text_layout.block_offset())
        })
        .unwrap_or(0.0);
    let pos = Point {
        x: pos.x + f64::from(x_offset),
        y: pos.y + f64::from(y_offset),
    };
    let transform = transform * Affine::translate((pos.x * scale, pos.y * scale));
    paint(
        scene,
        marker_layout,
        layout,
        doc,
        transform,
        0.0,
        0.0,
        scale,
        node_id,
        context,
        None,
    );
}

/// One bar of a decoration on a line of winkin-laid text, and the font
/// geometry it is drawn with, in device pixels along and across its line.
#[derive(Copy, Clone)]
pub(crate) struct WinkinDecoration {
    /// Where it starts and ends along the line.
    pub(crate) left: f64,
    pub(crate) right: f64,
    /// Its box's baseline, and its primary font's ascent and descent about it.
    pub(crate) baseline: f32,
    pub(crate) ascent: f32,
    pub(crate) descent: f32,
    /// Where an `auto` underline's top is under the baseline, and how thick
    /// an `auto` line is: Chrome's, as winkin works them out.
    pub(crate) underline_gap: f32,
    pub(crate) thickness: f32,
    /// Its box's used font size.
    pub(crate) font_size: f32,
    /// Whether the metrics are measured about a central baseline, as a
    /// vertical line's are.
    pub(crate) centered: bool,
}

/// Painting the decorations of lines laid out by winkin, with the same
/// resolution and drawing as the lines laid out by Parley.
impl DrawTextContext {
    /// Paints one bar of the decoration `styles` sets, as a decorating box:
    /// its underline and overline where `after_text` is false, and its line
    /// through where it is true. `color`, where given, is the color it is
    /// drawn in instead of its own: a shadow's.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_winkin_decoration(
        &mut self,
        scene: &mut impl PaintScene,
        transform: Affine,
        scale: f64,
        styles: &style::properties::ComputedValues,
        bar: WinkinDecoration,
        after_text: bool,
        color: Option<Color>,
    ) {
        let Some(mut deco) = super::resolve_decoration(styles) else {
            return;
        };
        if let Some(color) = color {
            deco.color = color;
        }
        self.deco_boxes.clear();
        self.deco_boxes.push(LineDecoration {
            node_id: NodeId::from_u64(0),
            deco,
            min_x: bar.left,
            max_x: bar.right,
            own: Some(DecorationRunGeometry {
                baseline: bar.baseline,
                ascent: bar.ascent,
                descent: bar.descent,
                // Placed from the baseline, up: an underline's top is its gap
                // under it.
                underline_offset: -bar.underline_gap,
                underline_size: bar.thickness,
                strikethrough_size: bar.thickness,
                // winkin's ascent is already the one the platform's browser
                // draws an overline against: its line metrics are.
                font: None,
                font_size: bar.font_size,
                css_font_size: f64::from(bar.font_size) / scale,
                centered: bar.centered,
            }),
            first: None,
        });
        let drawn = if after_text {
            TextDecorationLine::LINE_THROUGH
        } else {
            TextDecorationLine::UNDERLINE | TextDecorationLine::OVERLINE
        };
        super::flush_line_decorations(
            scene,
            transform,
            scale,
            &self.deco_boxes,
            &mut self.win_ascent_ratios,
            NodeId::from_u64(0),
            bar.baseline,
            drawn,
        );
        self.deco_boxes.clear();
    }
}
