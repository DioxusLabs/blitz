//! Native inline SVG painting.
//!
//! A natively rendered inline `<svg>` (`svg-native` feature) keeps
//! its subtree as live DOM nodes styled by Stylo. SVG layout
//! (`blitz_dom::svg`) has already resolved each element's transform, geometry
//! and stroke; painting walks the subtree, reads those results and the paint
//! properties from computed styles. Patterns, `<use>`, markers and filters
//! are not painted yet.

use anyrender::{MaskMode, PaintScene};
use blitz_dom::node::{ImageData, Node};
use blitz_dom::svg::{
    SvgClip, SvgGlyphRun, SvgGradient, SvgGradientShape, SvgLayoutKind, SvgMask, SvgPaintServer,
    SvgReference, SvgReferenceState, SvgSpreadMethod, SvgText, SvgTextDecorationKind, SvgViewport,
};
use color::{ColorSpaceTag, DynamicColor};
use kurbo::{Affine, BezPath, Rect, Shape, Stroke, Vec2};
use peniko::{BlendMode, ColorStop, Extend, Fill, Gradient, InterpolationAlphaSpace, StyleRef};
use style::computed_values::fill_rule::T as FillRule;
use style::computed_values::isolation::T as Isolation;
use style::computed_values::visibility::T as Visibility;
use style::properties::ComputedValues;
use style::values::computed::svg::{SVGOpacity, SVGPaint, SVGPaintKind};
use style::values::generics::svg::SVGPaintFallback;

use super::{ALMOST_OPAQUE, ElementCx};
use crate::blend::convert_blend_mode;
use crate::color::{Color, ToColorColor};

impl ElementCx<'_, '_> {
    /// The transform from a natively rendered inline `<svg>` root's user
    /// space to device pixels when its box is at `transform`, and its
    /// viewport, or `None` when it renders nothing.
    fn native_svg_root(&self, transform: Affine) -> Option<(Affine, SvgViewport, bool)> {
        let root = self.element.svg_root_data()?;
        // A zero-sized viewBox disables rendering of the element.
        let layout = root.layout.as_ref()?;
        if root.dims.degenerate_view_box {
            return None;
        }
        let content_box = self.frame.content_box();
        if content_box.width() <= 0.0 || content_box.height() <= 0.0 {
            return None;
        }
        // The content box is in device pixels and the root layout maps user
        // units onto CSS pixels.
        let transform = transform
            * Affine::translate(content_box.origin().to_vec2())
            * Affine::scale(self.scale)
            * layout.children_transform();
        let SvgLayoutKind::Viewport { viewport, .. } = layout.kind else {
            return None;
        };
        Some((transform, viewport, layout.has_blending))
    }

    /// Whether this is a natively rendered inline `<svg>` root, painted at
    /// `transform`, whose content stays within `clip` (in device pixels), so
    /// clipping its overflow can be skipped. Every draw under a clip pays
    /// for intersecting with it.
    pub(super) fn native_svg_within(&self, transform: Affine, clip: &impl Shape) -> bool {
        let [a, b, c, d, ..] = transform.as_coeffs();
        let Some(clip) = clip
            .as_rect()
            .filter(|_| b == 0.0 && c == 0.0 && a > 0.0 && d > 0.0)
        else {
            return false;
        };
        // Allow for rounding error in curve bounds, far too small to cover any
        // pixel outside the clip.
        let clip = transform.transform_rect_bbox(clip).inflate(1e-3, 1e-3);
        let Some((content_transform, ..)) = self.native_svg_root(transform) else {
            return false;
        };
        let ink = self
            .element
            .svg_root_data()
            .and_then(|root| root.layout.as_ref()?.ink);
        ink.is_none_or(|ink| clip.contains_rect(content_transform.transform_rect_bbox(ink)))
    }

    /// Paint a natively rendered inline `<svg>` root into its content box.
    pub(super) fn draw_native_svg(&self, scene: &mut impl PaintScene) {
        let Some((transform, viewport, has_blending)) = self.native_svg_root(self.transform) else {
            return;
        };
        // The outermost `<svg>` is an isolated group, so blending inside it
        // does not reach content behind it.
        // https://drafts.fxtf.org/compositing/#csscompositingrules_SVG
        if has_blending {
            scene.push_layer(
                Fill::NonZero,
                BlendMode::default(),
                1.0,
                Affine::IDENTITY,
                &self.viewport_rect(),
                None,
                None,
            );
        }
        let mut state = SvgReferenceState::default();
        self.paint_svg_children(scene, self.node, transform, viewport, &mut state);
        if has_blending {
            scene.pop_layer();
        }
    }

    /// The whole painted area in device pixels, for layers that must not clip.
    fn viewport_rect(&self) -> Rect {
        let painter = self.context;
        Rect::new(
            painter.initial_x,
            painter.initial_y,
            painter.initial_x + painter.width as f64,
            painter.initial_y + painter.height as f64,
        )
    }

    fn paint_svg_children(
        &self,
        scene: &mut impl PaintScene,
        node: &Node,
        transform: Affine,
        viewport: SvgViewport,
        state: &mut SvgReferenceState,
    ) {
        for child in node.children.iter() {
            self.paint_svg_node(scene, node.with(*child), transform, viewport, state);
        }
    }

    /// An isolated group covering `bounds` (in device pixels), for clip and
    /// mask layers to apply to.
    fn push_svg_group(
        &self,
        scene: &mut impl PaintScene,
        blend: BlendMode,
        alpha: f32,
        bounds: &Rect,
    ) {
        scene.push_layer(
            Fill::NonZero,
            blend,
            alpha.min(ALMOST_OPAQUE),
            Affine::IDENTITY,
            bounds,
            None,
            None,
        );
    }

    fn paint_svg_node(
        &self,
        scene: &mut impl PaintScene,
        node: &Node,
        parent_transform: Affine,
        viewport: SvgViewport,
        state: &mut SvgReferenceState,
    ) {
        let Some(layout) = node.element_data().and_then(|el| el.svg_layout()) else {
            return;
        };
        if matches!(
            layout.kind,
            SvgLayoutKind::NotRendered | SvgLayoutKind::Resource
        ) {
            return;
        }
        let Some(style) = node.svg_style() else {
            return;
        };
        let transform = parent_transform * layout.transform;

        // `clip-path` and `mask` apply in the element's own user space, which
        // for a nested `<svg>` is the one its `viewBox` establishes.
        let (effect_transform, effect_viewport) = match &layout.kind {
            SvgLayoutKind::Viewport {
                content_transform,
                viewport,
                ..
            } => (transform * *content_transform, *viewport),
            _ => (transform, viewport),
        };
        let bbox = layout.bbox;
        let dom = self.context.dom;
        state.start_element();
        let clip = match dom.resolve_svg_clip_path(node.id, &style, bbox, state) {
            SvgReference::None => None,
            SvgReference::Invalid => return,
            SvgReference::Valid(clip) => Some(clip),
        };
        let mask = match dom.resolve_svg_mask(node.id, &style, bbox, effect_viewport, state) {
            SvgReference::None => None,
            SvgReference::Invalid => return,
            SvgReference::Valid(mask) => Some(mask),
        };
        let simple_clip = clip
            .as_ref()
            .and_then(|clip| self.simple_svg_clip(node, clip));

        // Groups only cover what the element paints and the clip path and mask
        // region let through, as compositing a group costs its whole area.
        let mut bounds = self.viewport_rect();
        if let Some(clip) = &clip {
            bounds = bounds.intersect(self.svg_clip_bounds(node, clip, effect_transform));
        }
        if let Some(mask) = &mask {
            bounds = bounds.intersect(effect_transform.transform_rect_bbox(mask.rect));
        }

        // https://drafts.fxtf.org/compositing/#csscompositingrules_SVG
        let effects = style.get_effects();
        let blend = convert_blend_mode(effects.mix_blend_mode);
        let isolate = style.get_box().isolation == Isolation::Isolate;
        let grouped = effects.opacity < 1.0
            || isolate
            || blend != BlendMode::default()
            || mask.is_some()
            || (clip.is_some() && simple_clip.is_none());
        if grouped {
            let Some(ink) = self.svg_ink_bounds(node, parent_transform) else {
                return;
            };
            bounds = bounds.intersect(ink);
            if bounds.area() <= 0.0 {
                return;
            }
        } else if (clip.is_some() || mask.is_some()) && bounds.area() <= 0.0 {
            return;
        }
        let bounds = bounds.inflate(1.0, 1.0).expand();
        if grouped {
            if mask.is_some() || clip.is_some() {
                self.push_svg_group(scene, blend, effects.opacity, &bounds);
            } else {
                scene.push_layer(
                    Fill::NonZero,
                    blend,
                    effects.opacity,
                    Affine::IDENTITY,
                    &bounds,
                    None,
                    None,
                );
            }
        }
        if let Some((rule, clip_transform, path)) = &simple_clip {
            scene.push_clip_layer(*rule, effect_transform * *clip_transform, path);
        }

        match &layout.kind {
            SvgLayoutKind::NotRendered | SvgLayoutKind::Resource => {}
            SvgLayoutKind::Group => {
                self.paint_svg_children(scene, node, transform, viewport, state)
            }
            SvgLayoutKind::Switch { child } => {
                if let Some(child) = child {
                    self.paint_svg_node(scene, node.with(*child), transform, viewport, state);
                }
            }
            SvgLayoutKind::Viewport {
                content_transform,
                clip,
                viewport,
            } => {
                if let Some(clip) = clip {
                    scene.push_clip_layer(Fill::NonZero, transform, clip);
                }
                let content_transform = transform * *content_transform;
                self.paint_svg_children(scene, node, content_transform, *viewport, state);
                if clip.is_some() {
                    scene.pop_layer();
                }
            }
            SvgLayoutKind::Shape { path, stroke } => {
                if style.get_inherited_box().visibility == Visibility::Visible {
                    let target = PaintTarget {
                        style: &style,
                        bbox: layout.bbox,
                        viewport,
                    };
                    self.paint_shape(scene, &target, transform, path, stroke.as_ref());
                }
            }
            SvgLayoutKind::Text(text) => {
                let bbox = layout.bbox;
                self.paint_text(scene, node, text, transform, bbox, viewport);
            }
            SvgLayoutKind::Image {
                image,
                image_transform,
                clip,
                ..
            } => {
                if style.get_inherited_box().visibility == Visibility::Visible {
                    if let Some(clip) = clip {
                        scene.push_clip_layer(Fill::NonZero, transform, clip);
                    }
                    let image_transform = transform * *image_transform;
                    match image {
                        ImageData::Raster(image) => {
                            let rendering = style.get_inherited_box().image_rendering;
                            let quality = super::to_image_quality(rendering);
                            let brush = super::to_peniko_image(image, quality);
                            scene.draw_image(brush.as_ref(), image_transform);
                        }
                        #[cfg(feature = "svg-usvg")]
                        ImageData::Svg(svg) => {
                            anyrender_svg::render_svg_tree(scene, &svg.tree, image_transform);
                        }
                        ImageData::None => {}
                    }
                    if clip.is_some() {
                        scene.pop_layer();
                    }
                }
            }
        }

        if simple_clip.is_some() {
            scene.pop_layer();
        } else if let Some(clip) = &clip {
            self.apply_svg_clip(scene, node, clip, effect_transform, &bounds);
        }
        if let Some(mask) = &mask {
            let (transform, viewport) = (effect_transform, effect_viewport);
            self.apply_svg_mask(scene, node, mask, transform, viewport, &bounds, state);
        }
        if grouped {
            scene.pop_layer();
        }
    }

    /// A conservative bound, in device pixels, of what `node` paints when its
    /// parent's user space maps to device pixels by `parent_transform`, or
    /// `None` when it paints nothing.
    fn svg_ink_bounds(&self, node: &Node, parent_transform: Affine) -> Option<Rect> {
        let ink = node.element_data()?.svg_layout()?.ink_in_parent()?;
        Some(parent_transform.transform_rect_bbox(ink))
    }

    /// A clip path that one outline with one fill rule can express, as a
    /// fill rule, a transform to the referencing element's user space and
    /// that outline, so it can be a plain clip layer instead of a mask.
    ///
    /// That holds when neither the clip path nor its children have clip
    /// paths of their own, all children share a fill rule and their bounding
    /// boxes don't overlap, so concatenating their outlines is their union.
    /// `<text>` children contribute their glyph outlines.
    fn simple_svg_clip(&self, node: &Node, clip: &SvgClip) -> Option<(Fill, Affine, BezPath)> {
        const MAX_CHILDREN: usize = 16;
        if clip.clip.is_some() || clip.children.len() > MAX_CHILDREN {
            return None;
        }
        let mut rule = None;
        let mut outline = BezPath::new();
        let mut boxes: Vec<Rect> = Vec::with_capacity(clip.children.len());
        for child in &clip.children {
            if child.clip.is_some() {
                return None;
            }
            let layout = node.with(child.node).element_data()?.svg_layout()?;
            let (child_rule, path) = match &layout.kind {
                SvgLayoutKind::Shape { path, .. } if child.evenodd => (Fill::EvenOdd, path.clone()),
                SvgLayoutKind::Shape { path, .. } => (Fill::NonZero, path.clone()),
                SvgLayoutKind::Text(text) => (Fill::NonZero, svg_glyph_outlines(node, text)?),
                _ => continue,
            };
            if rule.is_some_and(|rule| rule != child_rule) {
                return None;
            }
            rule = Some(child_rule);
            let path = layout.transform * path;
            let bbox = path.bounding_box();
            let overlaps =
                |b: &Rect| b.x0 < bbox.x1 && bbox.x0 < b.x1 && b.y0 < bbox.y1 && bbox.y0 < b.y1;
            if boxes.iter().any(overlaps) {
                return None;
            }
            boxes.push(bbox);
            outline.extend(path);
        }
        Some((rule.unwrap_or(Fill::NonZero), clip.transform, outline))
    }

    /// The device-space bounds of the area that `clip` lets through, where
    /// `transform` maps the referencing element's user space to device space.
    fn svg_clip_bounds(&self, node: &Node, clip: &SvgClip, transform: Affine) -> Rect {
        let content_transform = transform * clip.transform;
        let mut bounds: Option<Rect> = None;
        for child in &clip.children {
            let child_node = node.with(child.node);
            let Some(layout) = child_node.element_data().and_then(|el| el.svg_layout()) else {
                continue;
            };
            let Some(mut bbox) = layout.bbox else {
                continue;
            };
            if matches!(layout.kind, SvgLayoutKind::Text(_)) {
                // Glyph outlines can overhang their cells.
                bbox = bbox.inflate(bbox.height(), bbox.height());
            }
            let child_bounds = (content_transform * layout.transform).transform_rect_bbox(bbox);
            bounds = Some(bounds.map_or(child_bounds, |b| b.union(child_bounds)));
        }
        bounds.unwrap_or(Rect::ZERO)
    }

    /// Clip the content painted since the enclosing group was pushed by
    /// `clip`, whose content `transform` maps to device space. `bounds` is
    /// the device-space area of that group.
    fn apply_svg_clip(
        &self,
        scene: &mut impl PaintScene,
        node: &Node,
        clip: &SvgClip,
        transform: Affine,
        bounds: &Rect,
    ) {
        scene.push_mask_layer(Fill::NonZero, MaskMode::Alpha, Affine::IDENTITY, bounds);
        if clip.clip.is_some() {
            self.push_svg_group(scene, BlendMode::default(), 1.0, bounds);
        }
        let content_transform = transform * clip.transform;
        for child in &clip.children {
            let child_node = node.with(child.node);
            let Some(layout) = child_node.element_data().and_then(|el| el.svg_layout()) else {
                continue;
            };
            let child_transform = content_transform * layout.transform;
            if child.clip.is_some() {
                self.push_svg_group(scene, BlendMode::default(), 1.0, bounds);
            }
            match &layout.kind {
                SvgLayoutKind::Shape { path, .. } => {
                    let rule = if child.evenodd {
                        Fill::EvenOdd
                    } else {
                        Fill::NonZero
                    };
                    scene.fill(rule, child_transform, Color::BLACK, None, path);
                }
                SvgLayoutKind::Text(text) => {
                    let paint = ShapePaint::Solid(Color::BLACK);
                    for run in &text.runs {
                        let visible = node.with(run.node).svg_style().is_some_and(|s| {
                            s.get_inherited_box().visibility == Visibility::Visible
                        });
                        if visible {
                            draw_svg_glyphs(scene, run, child_transform, &paint, None);
                        }
                    }
                }
                _ => {}
            }
            if let Some(child_clip) = &child.clip {
                self.apply_svg_clip(scene, node, child_clip, child_transform, bounds);
                scene.pop_layer();
            }
        }
        if let Some(own) = &clip.clip {
            self.apply_svg_clip(scene, node, own, transform, bounds);
            scene.pop_layer();
        }
        scene.pop_layer();
    }

    /// Mask the content painted since the enclosing group was pushed by
    /// `mask`, in the user space that `transform` maps to device space.
    /// `bounds` is the device-space area of that group.
    #[allow(clippy::too_many_arguments)]
    fn apply_svg_mask(
        &self,
        scene: &mut impl PaintScene,
        node: &Node,
        mask: &SvgMask,
        transform: Affine,
        viewport: SvgViewport,
        bounds: &Rect,
        state: &mut SvgReferenceState,
    ) {
        let mode = if mask.alpha {
            MaskMode::Alpha
        } else {
            MaskMode::Luminance
        };
        scene.push_mask_layer(Fill::NonZero, mode, transform, &mask.rect);
        if mask.mask.is_some() {
            self.push_svg_group(scene, BlendMode::default(), 1.0, bounds);
        }
        let mask_node = node.with(mask.node);
        state.push(mask.node);
        let content_transform = transform * mask.content_transform;
        self.paint_svg_children(scene, mask_node, content_transform, viewport, state);
        state.pop();
        if let Some(own) = &mask.mask {
            self.apply_svg_mask(scene, node, own, transform, viewport, bounds, state);
            scene.pop_layer();
        }
        scene.pop_layer();
    }

    /// Paint a `<text>`: underlines and overlines, then the glyphs, then
    /// line-throughs, each with the paint of the element it belongs to. Paint
    /// servers use the `<text>` element's bounding box.
    fn paint_text(
        &self,
        scene: &mut impl PaintScene,
        node: &Node,
        text: &SvgText,
        transform: Affine,
        bbox: Option<Rect>,
        viewport: SvgViewport,
    ) {
        let paint_decorations = |scene: &mut _, line_through: bool| {
            for deco in &text.decorations {
                if (deco.kind == SvgTextDecorationKind::LineThrough) != line_through {
                    continue;
                }
                let Some(style) = node.with(deco.node).svg_style() else {
                    continue;
                };
                if style.get_inherited_box().visibility != Visibility::Visible {
                    continue;
                }
                let target = PaintTarget {
                    style: &style,
                    bbox,
                    viewport,
                };
                let path = deco.rect.to_path(0.1);
                self.paint_shape(scene, &target, transform, &path, deco.stroke.as_ref());
            }
        };
        paint_decorations(scene, false);
        for run in &text.runs {
            let Some(style) = node.with(run.node).svg_style() else {
                continue;
            };
            if style.get_inherited_box().visibility != Visibility::Visible {
                continue;
            }
            let target = PaintTarget {
                style: &style,
                bbox,
                viewport,
            };
            // `opacity` on a `<tspan>` or `<a>` applies to its glyphs (the
            // `<text>` element's own opacity is applied as a group).
            let mut alpha = 1.0;
            let mut current = Some(run.node);
            while let Some(id) = current
                && id != node.id
            {
                let span = node.with(id);
                alpha *= span.svg_style().map_or(1.0, |s| s.get_effects().opacity);
                current = span.parent;
            }
            if alpha < 1.0 {
                let Some(ink) = run.ink else {
                    continue;
                };
                let bounds = self
                    .viewport_rect()
                    .intersect(transform.transform_rect_bbox(ink))
                    .inflate(1.0, 1.0)
                    .expand();
                scene.push_layer(
                    Fill::NonZero,
                    BlendMode::default(),
                    alpha,
                    Affine::IDENTITY,
                    &bounds,
                    None,
                    None,
                );
            }
            let svg = style.get_inherited_svg();
            if let Some(paint) = self.svg_paint(&target, &svg.fill, &svg.fill_opacity) {
                draw_svg_glyphs(scene, run, transform, &paint, None);
            }
            if let Some(stroke) = &run.stroke
                && let Some(paint) = self.svg_paint(&target, &svg.stroke, &svg.stroke_opacity)
            {
                draw_svg_glyphs(scene, run, transform, &paint, Some(stroke));
            }
            if alpha < 1.0 {
                scene.pop_layer();
            }
        }
        paint_decorations(scene, true);
    }
}

/// Draw a run's glyphs, batching consecutive glyphs that share a rotation
/// and scale. A gradient's brush transform is folded into the scene transform
/// (`draw_glyphs` has no brush transform), with glyph positions and outlines
/// mapped back through its inverse.
fn draw_svg_glyphs(
    scene: &mut impl PaintScene,
    run: &SvgGlyphRun,
    transform: Affine,
    paint: &ShapePaint,
    stroke: Option<&Stroke>,
) {
    if run.font_size <= 0.0 {
        return;
    }
    let brush_transform = match paint {
        ShapePaint::Solid(_) => Affine::IDENTITY,
        ShapePaint::Gradient(_, brush_transform) => *brush_transform,
    };
    if brush_transform.determinant().abs() < 1e-12 {
        return;
    }
    let inverse = brush_transform.inverse();
    let inverse_linear = inverse.with_translation(Vec2::ZERO);
    let skew = run
        .skew
        .map(|angle| Affine::skew((angle as f64).to_radians().tan(), 0.0));
    let coords: &[anyrender::NormalizedCoord] = bytemuck::cast_slice(&run.normalized_coords);
    for batch in run
        .glyphs
        .chunk_by(|a, b| a.rotate == b.rotate && a.scale_x == b.scale_x)
    {
        let outline = match (batch[0].transform(), skew) {
            (Some(t), Some(s)) => Some(t * s),
            (t, s) => t.or(s),
        };
        let glyph_transform = if brush_transform == Affine::IDENTITY {
            outline
        } else {
            Some(inverse_linear * outline.unwrap_or_default())
        };
        let brush: anyrender::PaintRef = match paint {
            ShapePaint::Solid(color) => (*color).into(),
            ShapePaint::Gradient(gradient, _) => gradient.into(),
        };
        let style: StyleRef = match stroke {
            Some(stroke) => stroke.into(),
            None => Fill::NonZero.into(),
        };
        scene.draw_glyphs(
            &run.font,
            run.font_size,
            false,
            coords,
            Vec2::ZERO,
            style,
            brush,
            1.0,
            transform * brush_transform,
            glyph_transform,
            batch.iter().map(|glyph| {
                let p = inverse * kurbo::Point::new(glyph.x as f64, glyph.y as f64);
                anyrender::Glyph {
                    id: glyph.id,
                    x: p.x as f32,
                    y: p.y as f32,
                }
            }),
        );
    }
}

/// The unhinted glyph outlines of the visible runs of `text` in its user
/// space, placed as [`draw_svg_glyphs`] places them. `None` when the runs use
/// more than one font, as outlines from different fonts may wind in opposite
/// directions and cancel out under the nonzero rule.
fn svg_glyph_outlines(node: &Node, text: &SvgText) -> Option<BezPath> {
    use skrifa::MetadataProvider as _;
    use skrifa::instance::{LocationRef, Size};
    use skrifa::outline::DrawSettings;
    use skrifa::raw::types::F2Dot14;

    struct Pen(BezPath);
    impl skrifa::outline::OutlinePen for Pen {
        fn move_to(&mut self, x: f32, y: f32) {
            self.0.move_to((x as f64, y as f64));
        }
        fn line_to(&mut self, x: f32, y: f32) {
            self.0.line_to((x as f64, y as f64));
        }
        fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
            self.0
                .quad_to((cx0 as f64, cy0 as f64), (x as f64, y as f64));
        }
        fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
            self.0.curve_to(
                (cx0 as f64, cy0 as f64),
                (cx1 as f64, cy1 as f64),
                (x as f64, y as f64),
            );
        }
        fn close(&mut self) {
            self.0.close_path();
        }
    }

    let mut outline = BezPath::new();
    let mut font = None;
    for run in &text.runs {
        let visible = node
            .with(run.node)
            .svg_style()
            .is_some_and(|s| s.get_inherited_box().visibility == Visibility::Visible);
        if !visible || run.font_size <= 0.0 {
            continue;
        }
        let id = (run.font.data.id(), run.font.index);
        if font.is_some_and(|font| font != id) {
            return None;
        }
        font = Some(id);
        let font = skrifa::FontRef::from_index(run.font.data.data(), run.font.index).ok()?;
        let glyphs = font.outline_glyphs();
        let coords: Vec<F2Dot14> = run
            .normalized_coords
            .iter()
            .map(|&c| F2Dot14::from_bits(c))
            .collect();
        let skew = run
            .skew
            .map(|angle| Affine::skew((angle as f64).to_radians().tan(), 0.0));
        for glyph in &run.glyphs {
            let Some(glyph_outline) = glyphs.get(skrifa::GlyphId::new(glyph.id)) else {
                continue;
            };
            let mut pen = Pen(BezPath::new());
            let settings =
                DrawSettings::unhinted(Size::new(run.font_size), LocationRef::new(&coords));
            glyph_outline.draw(settings, &mut pen).ok()?;
            let glyph_transform = match (glyph.transform(), skew) {
                (Some(t), Some(s)) => t * s,
                (t, s) => t.or(s).unwrap_or_default(),
            };
            let transform = Affine::translate((glyph.x as f64, glyph.y as f64))
                * glyph_transform
                * Affine::FLIP_Y;
            outline.extend(transform * pen.0);
        }
    }
    Some(outline)
}

/// The element an SVG paint is resolved for.
struct PaintTarget<'a> {
    style: &'a ComputedValues,
    bbox: Option<Rect>,
    viewport: SvgViewport,
}

/// A resolved fill or stroke paint: a brush and its transform relative to the
/// shape's user space.
enum ShapePaint {
    Solid(Color),
    Gradient(Gradient, Affine),
}

impl ElementCx<'_, '_> {
    /// Resolve an SVG paint, or `None` when nothing is painted.
    fn svg_paint(
        &self,
        target: &PaintTarget,
        paint: &SVGPaint,
        opacity: &SVGOpacity,
    ) -> Option<ShapePaint> {
        let style = target.style;
        let alpha = match opacity {
            SVGOpacity::Opacity(opacity) => *opacity,
            SVGOpacity::ContextFillOpacity | SVGOpacity::ContextStrokeOpacity => 1.0,
        };
        let solid = |color| {
            Some(ShapePaint::Solid(
                style
                    .resolve_color(color)
                    .as_srgb_color()
                    .multiply_alpha(alpha),
            ))
        };
        match &paint.kind {
            SVGPaintKind::None | SVGPaintKind::ContextFill | SVGPaintKind::ContextStroke => None,
            SVGPaintKind::Color(color) => solid(color),
            SVGPaintKind::PaintServer(url) => {
                match self
                    .context
                    .dom
                    .resolve_svg_paint_server(url, target.bbox, target.viewport)
                {
                    Some(SvgPaintServer::None) => None,
                    Some(SvgPaintServer::Color(color)) => Some(ShapePaint::Solid(
                        color.as_srgb_color().multiply_alpha(alpha),
                    )),
                    Some(SvgPaintServer::Gradient(gradient)) => {
                        Some(gradient_paint(&gradient, alpha))
                    }
                    // An invalid reference uses the fallback, if any.
                    None => match &paint.fallback {
                        SVGPaintFallback::Color(color) => solid(color),
                        SVGPaintFallback::None | SVGPaintFallback::Unset => None,
                    },
                }
            }
        }
    }

    fn paint_shape(
        &self,
        scene: &mut impl PaintScene,
        target: &PaintTarget,
        transform: Affine,
        path: &BezPath,
        stroke: Option<&Stroke>,
    ) {
        let svg = target.style.get_inherited_svg();
        if let Some(paint) = self.svg_paint(target, &svg.fill, &svg.fill_opacity) {
            let rule = match svg.fill_rule {
                FillRule::Nonzero => Fill::NonZero,
                FillRule::Evenodd => Fill::EvenOdd,
            };
            match &paint {
                ShapePaint::Solid(color) => scene.fill(rule, transform, *color, None, path),
                ShapePaint::Gradient(gradient, brush_transform) => {
                    scene.fill(rule, transform, gradient, Some(*brush_transform), path)
                }
            }
        }
        if let Some(stroke) = stroke
            && let Some(paint) = self.svg_paint(target, &svg.stroke, &svg.stroke_opacity)
        {
            match &paint {
                ShapePaint::Solid(color) => scene.stroke(stroke, transform, *color, None, path),
                ShapePaint::Gradient(gradient, brush_transform) => {
                    scene.stroke(stroke, transform, gradient, Some(*brush_transform), path)
                }
            }
        }
    }
}

fn gradient_paint(gradient: &SvgGradient, alpha: f32) -> ShapePaint {
    // A padded concentric gradient with a focal radius is a plain radial
    // gradient whose stops start at `fr / r`.
    let (shape, start_offset) = match gradient.shape {
        SvgGradientShape::Radial {
            focal,
            focal_radius,
            center,
            radius,
        } if focal == center
            && focal_radius > 0.0
            && focal_radius < radius
            && gradient.spread == SvgSpreadMethod::Pad =>
        {
            let shape = SvgGradientShape::Radial {
                focal,
                focal_radius: 0.0,
                center,
                radius,
            };
            (shape, (focal_radius / radius) as f32)
        }
        shape => (shape, 0.0),
    };
    let stops: Vec<ColorStop> = gradient
        .stops
        .iter()
        .map(|stop| ColorStop {
            offset: start_offset + stop.offset * (1.0 - start_offset),
            color: DynamicColor::from_alpha_color(stop.color.as_srgb_color().multiply_alpha(alpha)),
        })
        .collect();
    let mut brush = match shape {
        SvgGradientShape::Linear { start, end } => Gradient::new_linear(start, end),
        SvgGradientShape::Radial {
            focal,
            focal_radius,
            center,
            radius,
        } if focal == center && focal_radius == 0.0 => Gradient::new_radial(center, radius as f32),
        SvgGradientShape::Radial {
            focal,
            focal_radius,
            center,
            radius,
        } => Gradient::new_two_point_radial(focal, focal_radius as f32, center, radius as f32),
    }
    .with_stops(stops.as_slice())
    .with_interpolation_alpha_space(InterpolationAlphaSpace::Unpremultiplied)
    .with_extend(match gradient.spread {
        SvgSpreadMethod::Pad => Extend::Pad,
        SvgSpreadMethod::Reflect => Extend::Reflect,
        SvgSpreadMethod::Repeat => Extend::Repeat,
    });
    if gradient.linear_rgb {
        brush.interpolation_cs = ColorSpaceTag::LinearSrgb;
    }
    ShapePaint::Gradient(brush, gradient.transform)
}
