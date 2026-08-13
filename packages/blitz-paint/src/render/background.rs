use super::{ElementCx, to_image_quality, to_peniko_image};
use crate::color::{Color, ToColorColor};
use crate::gradient::to_peniko_gradient;
use anyrender::PaintScene;
use blitz_dom::node::{ImageData, ImageResourceData, SpecialElementData};
use kurbo::{self, Affine, BezPath, Point, Rect, Shape, Size, Vec2};
use peniko::{self, Fill};
use style::{
    properties::{
        generated::longhands::{
            background_attachment::single_value::computed_value::T as StyloBackgroundAttachment,
            background_clip::single_value::computed_value::T as StyloBackgroundClip,
            background_origin::single_value::computed_value::T as StyloBackgroundOrigin,
            mask_origin::single_value::computed_value::T as StyloMaskOrigin,
        },
        style_structs::{Background, SVG},
    },
    values::{
        computed::{
            BackgroundRepeat, Gradient as StyloGradient, Image as ComputedImage, LengthPercentage,
            background::BackgroundSize,
        },
        generics::image::GenericImage,
        specified::background::BackgroundRepeatKeyword,
    },
};

#[cfg(feature = "tracing")]
use tracing::warn;

/// A box from the CSS box model. Abstracts over the (structurally identical)
/// computed value types of the `background-clip`/`background-origin` and
/// `mask-clip`/`mask-origin` properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)] // The variants are named after the CSS keywords
pub(super) enum BoxModelBox {
    BorderBox,
    PaddingBox,
    ContentBox,
}

// Also covers MaskClip as the type is the same
impl From<StyloBackgroundClip> for BoxModelBox {
    fn from(value: StyloBackgroundClip) -> Self {
        match value {
            StyloBackgroundClip::BorderBox => Self::BorderBox,
            StyloBackgroundClip::PaddingBox => Self::PaddingBox,
            StyloBackgroundClip::ContentBox => Self::ContentBox,

            // TODO: support BorderArea
            StyloBackgroundClip::BorderArea => Self::BorderBox,
        }
    }
}

impl From<StyloBackgroundOrigin> for BoxModelBox {
    fn from(value: StyloBackgroundOrigin) -> Self {
        match value {
            StyloBackgroundOrigin::BorderBox => Self::BorderBox,
            StyloBackgroundOrigin::PaddingBox => Self::PaddingBox,
            StyloBackgroundOrigin::ContentBox => Self::ContentBox,
        }
    }
}

impl From<StyloMaskOrigin> for BoxModelBox {
    fn from(value: StyloMaskOrigin) -> Self {
        match value {
            StyloMaskOrigin::BorderBox => Self::BorderBox,
            StyloMaskOrigin::PaddingBox => Self::PaddingBox,
            StyloMaskOrigin::ContentBox => Self::ContentBox,
        }
    }
}

/// The styles and image data for a single layer of a CSS image layer list
/// (`background-image` or `mask-image`). The `background-*` and `mask-*`
/// properties share computed value types, which allows the layer painting code
/// to be shared.
pub(super) struct ImageLayerStyles<'a> {
    /// The computed value of the `background-image`/`mask-image` layer
    pub stylo_image: &'a ComputedImage,
    /// The loaded image resource if `stylo_image` is a `url()` image
    pub image_data: Option<&'a ImageResourceData>,
    pub position_x: &'a LengthPercentage,
    pub position_y: &'a LengthPercentage,
    pub repeat: &'a BackgroundRepeat,
    pub size: &'a BackgroundSize,
    pub clip: BoxModelBox,
    pub origin: BoxModelBox,
    pub attachment: StyloBackgroundAttachment,
}

impl<'a> ImageLayerStyles<'a> {
    pub(super) fn from_background(
        bg_styles: &'a Background,
        image_data: &'a [Option<ImageResourceData>],
        idx: usize,
    ) -> Self {
        Self {
            stylo_image: &bg_styles.background_image.0[idx],
            image_data: image_data.get(idx).and_then(Option::as_ref),
            position_x: get_cyclic(&bg_styles.background_position_x.0, idx),
            position_y: get_cyclic(&bg_styles.background_position_y.0, idx),
            repeat: get_cyclic(&bg_styles.background_repeat.0, idx),
            size: get_cyclic(&bg_styles.background_size.0, idx),
            clip: (*get_cyclic(&bg_styles.background_clip.0, idx)).into(),
            origin: (*get_cyclic(&bg_styles.background_origin.0, idx)).into(),
            attachment: *get_cyclic(&bg_styles.background_attachment.0, idx),
        }
    }

    pub(super) fn from_svg(
        svg_styles: &'a SVG,
        image_data: &'a [Option<ImageResourceData>],
        idx: usize,
    ) -> Self {
        Self {
            stylo_image: &svg_styles.mask_image.0[idx],
            image_data: image_data.get(idx).and_then(Option::as_ref),
            position_x: get_cyclic(&svg_styles.mask_position_x.0, idx),
            position_y: get_cyclic(&svg_styles.mask_position_y.0, idx),
            repeat: get_cyclic(&svg_styles.mask_repeat.0, idx),
            size: get_cyclic(&svg_styles.mask_size.0, idx),
            clip: (*get_cyclic(&svg_styles.mask_clip.0, idx)).into(),
            origin: (*get_cyclic(&svg_styles.mask_origin.0, idx)).into(),
            // There is no `mask-attachment` property
            attachment: StyloBackgroundAttachment::Scroll,
        }
    }
}

impl ElementCx<'_, '_> {
    pub(super) fn draw_background(&self, scene: &mut impl PaintScene) {
        let bg_styles = &self.style.get_background();
        let image_data = &self.element.background_images;
        let layer_count = bg_styles.background_image.0.len();

        // The background color is clipped by the clip of the last layer in the list
        let background_clip: BoxModelBox =
            (*get_cyclic(&bg_styles.background_clip.0, layer_count - 1)).into();
        let background_clip_path = self.box_path(background_clip);

        // Draw background color (if any)
        self.draw_solid_bg(scene, &background_clip_path);

        for idx in (0..layer_count).rev() {
            let layer = ImageLayerStyles::from_background(bg_styles, image_data, idx);
            let background_clip_path = self.box_path(layer.clip);

            self.context.layer_manager.maybe_with_layer(
                scene,
                true,
                1.0,
                self.transform,
                &background_clip_path,
                None,
                None,
                |scene| {
                    self.draw_image_layer(scene, &layer);
                },
            );
        }
    }

    /// The path of the given CSS box model box for this element
    pub(super) fn box_path(&self, css_box: BoxModelBox) -> BezPath {
        match css_box {
            BoxModelBox::BorderBox => self.frame.border_box_path(),
            BoxModelBox::PaddingBox => self.frame.padding_box_path(),
            BoxModelBox::ContentBox => self.frame.content_box_path(),
        }
    }

    /// The rect of the given CSS box model box for this element
    fn box_rect(&self, css_box: BoxModelBox) -> Rect {
        match css_box {
            BoxModelBox::BorderBox => self.frame.border_box,
            BoxModelBox::PaddingBox => self.frame.padding_box,
            BoxModelBox::ContentBox => self.frame.content_box,
        }
    }

    /// Draw a single layer of a CSS image layer list (`background-image` or `mask-image`)
    pub(super) fn draw_image_layer(&self, scene: &mut impl PaintScene, layer: &ImageLayerStyles) {
        match layer.stylo_image {
            GenericImage::None => {
                // Do nothing
            }
            GenericImage::Gradient(gradient) => self.draw_gradient_layer(scene, gradient, layer),
            GenericImage::Url(_) => {
                self.draw_raster_image_layer(scene, layer);
                #[cfg(feature = "svg")]
                self.draw_svg_image_layer(scene, layer);
            }
            GenericImage::LightDark(_) => {
                #[cfg(feature = "tracing")]
                warn!("Implement image layer drawing for ImageLightDark")
            }
            GenericImage::PaintWorklet(_) => {
                #[cfg(feature = "tracing")]
                warn!("Implement image layer drawing for Image::PaintWorklet")
            }
            GenericImage::CrossFade(_) => {
                #[cfg(feature = "tracing")]
                warn!("Implement image layer drawing for Image::CrossFade")
            }
            GenericImage::Image(_) => {
                #[cfg(feature = "tracing")]
                warn!("Implement image layer drawing for Image::Image")
            }
            GenericImage::ImageSet(_) => {
                #[cfg(feature = "tracing")]
                warn!("Implement image layer drawing for Image::ImageSet")
            }
        }
    }

    pub(super) fn draw_table_row_backgrounds(&self, scene: &mut impl PaintScene) {
        let SpecialElementData::TableRoot(table) = &self.element.special_data else {
            return;
        };
        let Some(grid_info) = &mut *table.computed_grid_info.borrow_mut() else {
            return;
        };

        let cols = &grid_info.columns;
        let inner_width =
            (cols.sizes.iter().sum::<f32>() + cols.gutters.iter().sum::<f32>()) as f64;

        let rows = &grid_info.rows;
        let mut y = rows.gutters.first().copied().unwrap_or_default() as f64;
        for ((row, &height), &gutter) in table
            .rows
            .iter()
            .zip(rows.sizes.iter())
            .zip(rows.gutters.iter().skip(1))
        {
            let row_node = &self.context.dom.get_node(row.node_id).unwrap();
            let Some(style) = row_node.primary_styles() else {
                continue;
            };

            let shape =
                Rect::new(0.0, y, inner_width, y + height as f64).scale_from_origin(self.scale);

            let current_color = style.clone_color();
            let background_color = &style.get_background().background_color;
            let bg_color = background_color
                .resolve_to_absolute(&current_color)
                .as_srgb_color();

            if bg_color != Color::TRANSPARENT {
                // Fill the color
                scene.fill(Fill::NonZero, self.transform, bg_color, None, &shape);
            }

            y += (height + gutter) as f64;
        }
    }

    fn draw_solid_bg(&self, scene: &mut impl PaintScene, shape: &BezPath) {
        let current_color = self.style.clone_color();
        let background_color = &self.style.get_background().background_color;
        let bg_color = background_color
            .resolve_to_absolute(&current_color)
            .as_srgb_color();

        if bg_color != Color::TRANSPARENT {
            // Fill the color
            scene.fill(Fill::NonZero, self.transform, bg_color, None, shape);
        }
    }

    /// Whether the layer is positioned against the viewport
    /// (`background-attachment: fixed`) rather than the element's origin box.
    /// `fixed` behaves as `scroll` on elements affected by a CSS transform
    /// (the transformed element acts as the layer's containing block).
    fn layer_is_fixed(&self, layer: &ImageLayerStyles) -> bool {
        layer.attachment == StyloBackgroundAttachment::Fixed && !self.is_transformed()
    }

    /// The background positioning area and the transform from its coordinate
    /// space to the scene for a fixed layer: the viewport, unaffected by any
    /// scrolling.
    fn fixed_positioning_area(&self) -> (Rect, Affine) {
        let viewport_rect = Rect::new(
            0.0,
            0.0,
            self.context.width as f64,
            self.context.height as f64,
        );
        let transform = Affine::translate((self.context.initial_x, self.context.initial_y));
        (viewport_rect, transform)
    }

    /// Whether this element or any of its ancestors has a CSS transform
    ///
    /// TODO: this misses transformed elements whose resolved transform is the
    /// identity (e.g. `transform: translate(0)`), and elements with
    /// `will-change: transform`, both of which should also degrade `fixed`
    /// to `scroll` (see WPT css/css-transforms/transform-fixed-bg-005/008)
    fn is_transformed(&self) -> bool {
        let mut current = Some(self.node.id);
        while let Some(node) = current.and_then(|id| self.context.dom.get_node(id)) {
            if node.transform().is_some() {
                return true;
            }
            current = node.parent;
        }
        false
    }

    #[cfg(feature = "svg")]
    fn draw_svg_image_layer(&self, scene: &mut impl PaintScene, layer: &ImageLayerStyles) {
        use kurbo::Affine;

        let Some(bg_image) = layer.image_data else {
            return;
        };
        let ImageData::Svg(svg) = &bg_image.image else {
            return;
        };

        let (area_rect, base_transform) = if self.layer_is_fixed(layer) {
            self.fixed_positioning_area()
        } else {
            (self.frame.padding_box, self.transform)
        };

        let frame_w = (area_rect.width() / self.scale) as f32;
        let frame_h = (area_rect.height() / self.scale) as f32;

        let svg_size = svg.tree.size();

        // Compute the SVG's concrete object size per the CSS default sizing
        // algorithm. usvg resolves an SVG that lacks explicit `width`/`height`
        // to its `viewBox` size, but such an image has only an intrinsic aspect
        // ratio (no intrinsic dimensions). Passing this size to
        // `compute_layer_size` for the `background-size: auto` case supplies
        // both the concrete size (used verbatim for `auto`) and the aspect
        // ratio (used by `cover`/`contain` and single-`auto` sizes).
        let aspect_ratio = svg.aspect_ratio();
        let (object_w, object_h) = match (svg.intrinsic_width(), svg.intrinsic_height()) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, w / aspect_ratio),
            (None, Some(h)) => (h * aspect_ratio, h),
            (None, None) => {
                // No intrinsic dimensions: scale the aspect ratio to fit
                // ("contain") within the background positioning area.
                let scale = (frame_w / svg_size.width()).min(frame_h / svg_size.height());
                (svg_size.width() * scale, svg_size.height() * scale)
            }
        };

        let bg_size = compute_layer_size(
            layer,
            frame_w,
            frame_h,
            BackgroundSizeComputeMode::Size(object_w, object_h),
        );

        let x_ratio = (bg_size.width / svg_size.width() as f64) * self.scale;
        let y_ratio = (bg_size.height / svg_size.height() as f64) * self.scale;

        let bg_pos = compute_layer_position(
            layer,
            frame_w - bg_size.width as f32,
            frame_h - bg_size.height as f32,
        );

        let transform = base_transform
            * kurbo::Affine::translate((bg_pos.x * self.scale, bg_pos.y * self.scale))
            * Affine::scale_non_uniform(x_ratio, y_ratio);

        anyrender_svg::render_svg_tree(scene, &svg.tree, transform);
    }

    fn draw_raster_image_layer(&self, scene: &mut impl PaintScene, layer: &ImageLayerStyles) {
        let Some(bg_image) = layer.image_data else {
            return;
        };
        let ImageData::Raster(image_data) = &bg_image.image else {
            return;
        };

        let image_rendering = self.style.clone_image_rendering();
        let quality = to_image_quality(image_rendering);

        let (origin_rect, base_transform) = if self.layer_is_fixed(layer) {
            self.fixed_positioning_area()
        } else {
            (self.box_rect(layer.origin), self.transform)
        };

        let image_width = image_data.width as f64;
        let image_height = image_data.height as f64;

        let (bg_pos, bg_size) = compute_layer_position_and_size(
            layer,
            origin_rect.width() / self.scale,
            origin_rect.height() / self.scale,
            BackgroundSizeComputeMode::Size(image_width as f32, image_height as f32),
        );

        let bg_pos = (bg_pos.to_vec2() * self.scale).to_point();
        let bg_size = bg_size * self.scale;

        let x_ratio = bg_size.width / image_width;
        let y_ratio = bg_size.height / image_height;

        let BackgroundRepeat(repeat_x, repeat_y) = layer.repeat;

        let x = raster_axis_tiling(
            *repeat_x,
            origin_rect.x0,
            origin_rect.width(),
            bg_pos.x,
            bg_size.width,
            image_width,
            x_ratio,
        );
        let y = raster_axis_tiling(
            *repeat_y,
            origin_rect.y0,
            origin_rect.height(),
            bg_pos.y,
            bg_size.height,
            image_height,
            y_ratio,
        );

        let transform = base_transform
            .pre_scale_non_uniform(x_ratio, y_ratio)
            .then_translate(Vec2 {
                x: x.translate,
                y: y.translate,
            });
        let tile_rect = Rect::new(0.0, 0.0, x.rect_len, y.rect_len);

        for hc in 0..y.count {
            for wc in 0..x.count {
                let transform = transform.then_translate(Vec2 {
                    x: wc as f64 * x.stride,
                    y: hc as f64 * y.stride,
                });

                scene.fill(
                    peniko::Fill::NonZero,
                    transform,
                    to_peniko_image(image_data, quality).as_ref(),
                    None,
                    &tile_rect,
                );
            }
        }
    }

    fn draw_gradient_layer(
        &self,
        scene: &mut impl PaintScene,
        gradient: &StyloGradient,
        layer: &ImageLayerStyles,
    ) {
        // For a fixed layer the positioning area (the viewport) already covers
        // everything visible, so it also serves as the clip rect (no extension
        // towards the clip box is needed).
        let (origin_rect, base_transform, clip_rect) = if self.layer_is_fixed(layer) {
            let (viewport_rect, transform) = self.fixed_positioning_area();
            (viewport_rect, transform, viewport_rect)
        } else {
            (
                self.box_rect(layer.origin),
                self.transform,
                self.box_rect(layer.clip),
            )
        };

        let (bg_pos, bg_size) = compute_layer_position_and_size(
            layer,
            origin_rect.width() / self.scale,
            origin_rect.height() / self.scale,
            BackgroundSizeComputeMode::Auto,
        );

        let bg_pos = (bg_pos.to_vec2() * self.scale).to_point();
        let bg_size = bg_size * self.scale;

        let BackgroundRepeat(repeat_x, repeat_y) = layer.repeat;

        let x = gradient_axis_tiling(
            *repeat_x,
            origin_rect.x0,
            origin_rect.width(),
            clip_rect.x0,
            clip_rect.width(),
            bg_pos.x,
            bg_size.width,
        );
        let y = gradient_axis_tiling(
            *repeat_y,
            origin_rect.y0,
            origin_rect.height(),
            clip_rect.y0,
            clip_rect.height(),
            bg_pos.y,
            bg_size.height,
        );

        // FIXME: https://wpt.live/css/css-backgrounds/background-size/background-size-near-zero-gradient.html
        if x.count as u64 * y.count as u64 > 500 {
            return;
        }

        let tile_rect = Rect::new(0.0, 0.0, x.rect_len, y.rect_len);
        let bounding_box = self.frame.border_box.bounding_box();
        let current_color = self.style.clone_color();

        let (gradient, gradient_transform) = to_peniko_gradient(
            gradient,
            tile_rect,
            bounding_box,
            self.scale,
            &current_color,
        );
        let brush = anyrender::Paint::Gradient(&gradient);

        let transform = base_transform.then_translate(Vec2 {
            x: x.translate,
            y: y.translate,
        });

        for hc in 0..y.count {
            for wc in 0..x.count {
                let transform = transform.then_translate(Vec2 {
                    x: wc as f64 * x.stride,
                    y: hc as f64 * y.stride,
                });

                scene.fill(
                    peniko::Fill::NonZero,
                    transform,
                    brush.clone(),
                    gradient_transform,
                    &tile_rect,
                );
            }
        }
    }
}

fn compute_layer_position_and_size(
    layer: &ImageLayerStyles,
    container_w: f64,
    container_h: f64,
    size_mode: BackgroundSizeComputeMode,
) -> (Point, Size) {
    use BackgroundRepeatKeyword::*;

    let bg_size = compute_layer_size(layer, container_w as f32, container_h as f32, size_mode);

    let bg_pos = compute_layer_position(
        layer,
        (container_w - bg_size.width) as f32,
        (container_h - bg_size.height) as f32,
    );

    let BackgroundRepeat(repeat_x, repeat_y) = layer.repeat;

    let bg_size = if matches!(repeat_x, Round) && matches!(repeat_y, Round) {
        let count = (container_w / bg_size.width).round();
        let width = container_w / count;

        let count = (container_h / bg_size.height).round();
        let height = container_h / count;

        Size::new(width, height)
    } else if matches!(repeat_x, Round) {
        let count = (container_w / bg_size.width).round();
        let width = container_w / count;
        Size::new(width, bg_size.height)
    } else if matches!(repeat_y, Round) {
        let count = (container_h / bg_size.height).round();
        let height = container_h / count;
        Size::new(bg_size.width, height)
    } else {
        bg_size
    };

    (bg_pos, bg_size)
}

#[inline]
fn compute_layer_position(layer: &ImageLayerStyles, width: f32, height: f32) -> Point {
    use style::values::computed::Length;

    let bg_pos_x = layer.position_x.resolve(Length::new(width)).px() as f64;
    let bg_pos_y = layer.position_y.resolve(Length::new(height)).px() as f64;

    Point::new(bg_pos_x, bg_pos_y)
}

fn compute_layer_size(
    layer: &ImageLayerStyles,
    container_w: f32,
    container_h: f32,
    mode: BackgroundSizeComputeMode,
) -> kurbo::Size {
    use style::values::computed::Length;
    use style::values::generics::length::GenericLengthPercentageOrAuto as Lpa;

    let (width, height): (f32, f32) = match layer.size {
        BackgroundSize::ExplicitSize { width, height } => {
            let width = width.map(|w| w.0.resolve(Length::new(container_w)));
            let height = height.map(|h| h.0.resolve(Length::new(container_h)));

            match (width, height) {
                (Lpa::LengthPercentage(width), Lpa::LengthPercentage(height)) => {
                    let width = width.px();
                    let height = height.px();
                    match mode {
                        BackgroundSizeComputeMode::Auto => (width, height),
                        BackgroundSizeComputeMode::Size(_, _) => (width, height),
                    }
                }
                (Lpa::LengthPercentage(width), Lpa::Auto) => {
                    let width = width.px();
                    let height = match mode {
                        BackgroundSizeComputeMode::Auto => container_h,
                        BackgroundSizeComputeMode::Size(bg_w, bg_h) => bg_h / bg_w * width,
                    };
                    (width, height)
                }
                (Lpa::Auto, Lpa::LengthPercentage(height)) => {
                    let height = height.px();
                    let width = match mode {
                        BackgroundSizeComputeMode::Auto => container_w,
                        BackgroundSizeComputeMode::Size(bg_w, bg_h) => bg_w / bg_h * height,
                    };
                    (width, height)
                }
                (Lpa::Auto, Lpa::Auto) => match mode {
                    BackgroundSizeComputeMode::Auto => (container_w, container_h),
                    BackgroundSizeComputeMode::Size(bg_w, bg_h) => (bg_w, bg_h),
                },
            }
        }
        BackgroundSize::Cover => match mode {
            BackgroundSizeComputeMode::Auto => (container_w, container_h),
            BackgroundSizeComputeMode::Size(bg_w, bg_h) => {
                // Scale to the smallest size that covers both axes
                let ratio = (container_w / bg_w).max(container_h / bg_h);
                (bg_w * ratio, bg_h * ratio)
            }
        },
        BackgroundSize::Contain => match mode {
            BackgroundSizeComputeMode::Auto => (container_w, container_h),
            BackgroundSizeComputeMode::Size(bg_w, bg_h) => {
                // Scale to the largest size contained by both axes
                let ratio = (container_w / bg_w).min(container_h / bg_h);
                (bg_w * ratio, bg_h * ratio)
            }
        },
    };

    kurbo::Size {
        width: width as f64,
        height: height as f64,
    }
}

enum BackgroundSizeComputeMode {
    Auto,
    Size(f32, f32),
}

/// The placement and tiling of a background layer along one axis: a
/// translation applied to the layer as a whole, the length of each filled
/// rect, and the number of explicit tiles with the stride between them.
struct AxisTiling {
    /// Translation (in device pixels) positioning the first tile
    translate: f64,
    /// Length of each filled rect, in the coordinate space of the fill's transform
    rect_len: f64,
    /// Number of explicitly drawn tiles
    count: u32,
    /// Stride (in device pixels) between the starts of consecutive tiles
    stride: f64,
}

/// Per-axis placement and tiling for a raster image layer. `Repeat`/`Round`
/// produce a single fill covering the whole positioning area (relying on the
/// image brush repeating), while `Space` produces `count` explicit tiles
/// spaced `stride` apart.
///
/// The fill rect is in image pixel coordinates (the drawing transform is
/// pre-scaled by `ratio`), while translations are in device pixels.
fn raster_axis_tiling(
    repeat: BackgroundRepeatKeyword,
    origin_start: f64,
    origin_len: f64,
    bg_pos: f64,
    tile_len: f64,
    image_len: f64,
    ratio: f64,
) -> AxisTiling {
    use BackgroundRepeatKeyword::*;

    match repeat {
        Repeat | Round => {
            let extend_len = extend(bg_pos, tile_len);
            AxisTiling {
                translate: origin_start - extend_len,
                rect_len: (origin_len + extend_len) / ratio,
                count: 1,
                stride: 0.0,
            }
        }
        Space => {
            let (count, stride) = compute_space_count_and_stride(origin_len, tile_len);
            AxisTiling {
                translate: origin_start + if count == 1 { bg_pos } else { 0.0 },
                rect_len: image_len,
                count,
                stride,
            }
        }
        NoRepeat => AxisTiling {
            translate: origin_start + bg_pos,
            rect_len: image_len,
            count: 1,
            stride: 0.0,
        },
    }
}

/// Per-axis placement and tiling for a gradient layer. Unlike raster images,
/// gradients cannot rely on brush repetition, so `Repeat`/`Round` also produce
/// explicit tiles. When the clip box extends beyond the origin box, tiling
/// starts from the clip box edge so the pattern covers the whole clipped area.
fn gradient_axis_tiling(
    repeat: BackgroundRepeatKeyword,
    origin_start: f64,
    origin_len: f64,
    clip_start: f64,
    clip_len: f64,
    bg_pos: f64,
    tile_len: f64,
) -> AxisTiling {
    use BackgroundRepeatKeyword::*;

    match repeat {
        Repeat | Round => {
            // The clip and origin boxes are nested, so the clip box extends
            // beyond the origin box iff it does so at either end
            let clip_is_outer =
                clip_start < origin_start || clip_start + clip_len > origin_start + origin_len;
            let (area_start, area_len) = if clip_is_outer {
                (clip_start, clip_len)
            } else {
                (origin_start, origin_len)
            };
            let extend_len = extend((origin_start - area_start) + bg_pos, tile_len);
            let count = ((area_len + extend_len) / tile_len).ceil() as u32;
            AxisTiling {
                translate: area_start - extend_len,
                rect_len: tile_len,
                count,
                stride: tile_len,
            }
        }
        Space => {
            let (count, stride) = compute_space_count_and_stride(origin_len, tile_len);
            AxisTiling {
                translate: origin_start + if count == 1 { bg_pos } else { 0.0 },
                rect_len: tile_len,
                count,
                stride,
            }
        }
        NoRepeat => AxisTiling {
            translate: origin_start + bg_pos,
            rect_len: tile_len,
            count: 1,
            stride: 0.0,
        },
    }
}

fn compute_space_count_and_stride(bg_size: f64, size: f64) -> (u32, f64) {
    let modulo = bg_size % size;
    let count = (((bg_size - modulo) / size) as u32).max(1);
    let stride = if count > 1 {
        modulo / (count - 1) as f64
    } else {
        0.0
    } + size;

    (count, stride)
}

#[inline]
pub(super) fn get_cyclic<T>(values: &[T], layer_index: usize) -> &T {
    &values[layer_index % values.len()]
}

fn extend(offset: f64, length: f64) -> f64 {
    let extend_length = offset % length;
    if extend_length > 0.0 {
        length - extend_length
    } else {
        -extend_length
    }
}
