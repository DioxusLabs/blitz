use super::{ElementCx, to_image_quality, to_peniko_image};
use crate::color::{Color, ToColorColor};
use crate::gradient::to_peniko_gradient;
use anyrender::PaintScene;
use blitz_dom::node::{ImageData, ImageResourceData, SpecialElementData};
use kurbo::{self, BezPath, Point, Rect, Shape, Size, Vec2};
use peniko::{self, Fill};
use style::{
    properties::{
        generated::longhands::{
            background_clip::single_value::computed_value::T as StyloBackgroundClip,
            background_origin::single_value::computed_value::T as StyloBackgroundOrigin,
            mask_clip::single_value::computed_value::T as StyloMaskClip,
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

impl From<StyloBackgroundClip> for BoxModelBox {
    fn from(value: StyloBackgroundClip) -> Self {
        match value {
            StyloBackgroundClip::BorderBox => Self::BorderBox,
            StyloBackgroundClip::PaddingBox => Self::PaddingBox,
            StyloBackgroundClip::ContentBox => Self::ContentBox,
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

impl From<StyloMaskClip> for BoxModelBox {
    fn from(value: StyloMaskClip) -> Self {
        match value {
            StyloMaskClip::BorderBox => Self::BorderBox,
            StyloMaskClip::PaddingBox => Self::PaddingBox,
            StyloMaskClip::ContentBox => Self::ContentBox,
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

    #[cfg(feature = "svg")]
    fn draw_svg_image_layer(&self, scene: &mut impl PaintScene, layer: &ImageLayerStyles) {
        use kurbo::Affine;

        let Some(bg_image) = layer.image_data else {
            return;
        };
        let ImageData::Svg(svg) = &bg_image.image else {
            return;
        };

        let frame_w = (self.frame.padding_box.width() / self.scale) as f32;
        let frame_h = (self.frame.padding_box.height() / self.scale) as f32;

        let svg_size = svg.size();
        let bg_size = compute_layer_size(
            layer,
            frame_w,
            frame_h,
            BackgroundSizeComputeMode::Size(svg_size.width(), svg_size.height()),
        );

        let x_ratio = (bg_size.width / svg_size.width() as f64) * self.scale;
        let y_ratio = (bg_size.height / svg_size.height() as f64) * self.scale;

        let bg_pos = compute_layer_position(
            layer,
            frame_w - bg_size.width as f32,
            frame_h - bg_size.height as f32,
        );

        let transform = self.transform
            * kurbo::Affine::translate((bg_pos.x * self.scale, bg_pos.y * self.scale))
            * Affine::scale_non_uniform(x_ratio, y_ratio);

        anyrender_svg::render_svg_tree(scene, svg, transform);
    }

    fn draw_raster_image_layer(&self, scene: &mut impl PaintScene, layer: &ImageLayerStyles) {
        use BackgroundRepeatKeyword::*;

        let Some(bg_image) = layer.image_data else {
            return;
        };
        let ImageData::Raster(image_data) = &bg_image.image else {
            return;
        };

        let image_rendering = self.style.clone_image_rendering();
        let quality = to_image_quality(image_rendering);

        let origin_rect = self.box_rect(layer.origin);

        let image_width = image_data.width as f64;
        let image_height = image_data.height as f64;

        let (bg_pos, bg_size) = compute_layer_position_and_size(
            layer,
            origin_rect.width() / self.scale,
            origin_rect.height() / self.scale,
            BackgroundSizeComputeMode::Size(image_width as f32, image_height as f32),
        );

        let bg_pos_x = bg_pos.x * self.scale;
        let bg_pos_y = bg_pos.y * self.scale;
        let bg_size = bg_size * self.scale;

        let x_ratio = bg_size.width / image_width;
        let y_ratio = bg_size.height / image_height;

        let BackgroundRepeat(repeat_x, repeat_y) = layer.repeat;

        let transform = self.transform.pre_scale_non_uniform(x_ratio, y_ratio);
        let (origin_rect, transform) = match repeat_x {
            Repeat | Round => {
                let extend_width = extend(bg_pos_x, bg_size.width);

                let transform = transform.then_translate(Vec2 {
                    x: origin_rect.x0 - extend_width,
                    y: 0.0,
                });

                let origin_rect = origin_rect.with_size(Size::new(
                    (origin_rect.width() + extend_width) / x_ratio,
                    origin_rect.height(),
                ));

                (origin_rect, transform)
            }
            Space => (origin_rect, transform),
            NoRepeat => {
                let transform = transform.then_translate(Vec2 {
                    x: origin_rect.x0 + bg_pos_x,
                    y: 0.0,
                });

                let origin_rect =
                    origin_rect.with_size(Size::new(image_width, origin_rect.height()));

                (origin_rect, transform)
            }
        };
        let (origin_rect, transform) = match repeat_y {
            Repeat | Round => {
                let extend_height = extend(bg_pos_y, bg_size.height);

                let transform = transform.then_translate(Vec2 {
                    x: 0.0,
                    y: origin_rect.y0 - extend_height,
                });

                let origin_rect = origin_rect.with_size(Size::new(
                    origin_rect.width(),
                    (origin_rect.height() + extend_height) / y_ratio,
                ));

                (origin_rect, transform)
            }
            Space => (origin_rect, transform),
            NoRepeat => {
                let transform = transform.then_translate(Vec2 {
                    x: 0.0,
                    y: origin_rect.y0 + bg_pos_y,
                });

                let origin_rect =
                    origin_rect.with_size(Size::new(origin_rect.width(), image_height));
                (origin_rect, transform)
            }
        };

        if matches!(repeat_x, Space) || matches!(repeat_y, Space) {
            let (origin_rect, transform, width_count, width_gap) = if matches!(repeat_x, Space) {
                let (count, gap) = compute_space_count_and_gap(origin_rect.width(), bg_size.width);

                let transform = if count == 1 {
                    transform.then_translate(Vec2 {
                        x: bg_pos_x,
                        y: 0.0,
                    })
                } else {
                    transform
                };

                let origin_rect =
                    origin_rect.with_size(Size::new(image_width, origin_rect.height()));

                (origin_rect, transform, count, gap)
            } else {
                (origin_rect, transform, 1, 0.0)
            };

            let (origin_rect, transform, height_count, height_gap) = if matches!(repeat_y, Space) {
                let (count, gap) =
                    compute_space_count_and_gap(origin_rect.height(), bg_size.height);

                let transform = if count == 1 {
                    transform.then_translate(Vec2 {
                        x: 0.0,
                        y: bg_pos_y,
                    })
                } else {
                    transform
                };

                let origin_rect =
                    origin_rect.with_size(Size::new(origin_rect.width(), image_height));

                (origin_rect, transform, count, gap)
            } else {
                (origin_rect, transform, 1, 0.0)
            };

            for hc in 0..height_count {
                for wc in 0..width_count {
                    let width_gap = if matches!(repeat_x, Space) {
                        origin_rect.x0 + wc as f64 * width_gap
                    } else {
                        0.0
                    };

                    let height_gap = if matches!(repeat_y, Space) {
                        origin_rect.y0 + hc as f64 * height_gap
                    } else {
                        0.0
                    };

                    let transform = transform.then_translate(Vec2 {
                        x: width_gap,
                        y: height_gap,
                    });

                    scene.fill(
                        peniko::Fill::NonZero,
                        transform,
                        to_peniko_image(image_data, quality).as_ref(),
                        None,
                        &Rect::new(0.0, 0.0, origin_rect.width(), origin_rect.height()),
                    );
                }
            }
        } else {
            scene.fill(
                peniko::Fill::NonZero,
                transform,
                to_peniko_image(image_data, quality).as_ref(),
                None,
                &Rect::new(0.0, 0.0, origin_rect.width(), origin_rect.height()),
            );
        }
    }

    fn draw_gradient_layer(
        &self,
        scene: &mut impl PaintScene,
        gradient: &StyloGradient,
        layer: &ImageLayerStyles,
    ) {
        use BackgroundRepeatKeyword::*;

        let background_clip = layer.clip;
        let background_origin = layer.origin;
        let origin_rect = self.box_rect(background_origin);

        let (bg_pos, bg_size) = compute_layer_position_and_size(
            layer,
            origin_rect.width() / self.scale,
            origin_rect.height() / self.scale,
            BackgroundSizeComputeMode::Auto,
        );

        let bg_pos_x = bg_pos.x * self.scale;
        let bg_pos_y = bg_pos.y * self.scale;
        let bg_size = bg_size * self.scale;

        let BackgroundRepeat(repeat_x, repeat_y) = layer.repeat;

        let transform = self.transform;
        let (origin_rect, transform, width_count, width_gap) = match repeat_x {
            Repeat | Round => {
                let (origin_rect, extend_width, count) = if (background_clip, background_origin)
                    == (BoxModelBox::BorderBox, BoxModelBox::PaddingBox)
                {
                    let extend_width = extend(self.frame.border_width.x0 + bg_pos_x, bg_size.width);

                    let width = self.frame.border_box.width() + extend_width;
                    let count = (width / bg_size.width).ceil() as u32;

                    let origin_rect = Rect::from_origin_size(
                        Point::new(self.frame.border_box.x0, origin_rect.y0),
                        Size::new(bg_size.width, origin_rect.height()),
                    );

                    (origin_rect, extend_width, count)
                } else if (background_clip, background_origin)
                    == (BoxModelBox::BorderBox, BoxModelBox::ContentBox)
                {
                    let extend_width = extend(
                        self.frame.border_width.x0 + self.frame.padding_width.x0 + bg_pos_x,
                        bg_size.width,
                    );
                    let width = self.frame.border_box.width() + extend_width;
                    let count = (width / bg_size.width).ceil() as u32;

                    let origin_rect = Rect::from_origin_size(
                        Point::new(self.frame.border_box.x0, origin_rect.y0),
                        Size::new(bg_size.width, origin_rect.height()),
                    );

                    (origin_rect, extend_width, count)
                } else if (background_clip, background_origin)
                    == (BoxModelBox::PaddingBox, BoxModelBox::ContentBox)
                {
                    let extend_width =
                        extend(self.frame.padding_width.x0 + bg_pos_x, bg_size.width);
                    let width = self.frame.padding_box.width() + extend_width;
                    let count = (width / bg_size.width).ceil() as u32;

                    let origin_rect = Rect::from_origin_size(
                        Point::new(self.frame.padding_box.x0, origin_rect.y0),
                        Size::new(bg_size.width, origin_rect.height()),
                    );

                    (origin_rect, extend_width, count)
                } else {
                    let extend_width = extend(bg_pos_x, bg_size.width);
                    let width = origin_rect.width() + extend_width;
                    let count = (width / bg_size.width).ceil() as u32;
                    let origin_rect =
                        origin_rect.with_size(Size::new(bg_size.width, origin_rect.height()));

                    (origin_rect, extend_width, count)
                };

                let transform = transform.then_translate(Vec2 {
                    x: origin_rect.x0 - extend_width,
                    y: 0.0,
                });

                (origin_rect, transform, count, bg_size.width)
            }
            Space => {
                let (count, gap) = compute_space_count_and_gap(origin_rect.width(), bg_size.width);

                let transform = transform.then_translate(Vec2 {
                    x: origin_rect.x0 + if count == 1 { bg_pos_x } else { 0.0 },
                    y: 0.0,
                });

                let origin_rect =
                    origin_rect.with_size(Size::new(bg_size.width, origin_rect.height()));

                (origin_rect, transform, count, gap)
            }
            NoRepeat => {
                let transform = transform.then_translate(Vec2 {
                    x: origin_rect.x0 + bg_pos_x,
                    y: 0.0,
                });

                let origin_rect =
                    origin_rect.with_size(Size::new(bg_size.width, origin_rect.height()));

                (origin_rect, transform, 1, 0.0)
            }
        };
        let (origin_rect, transform, height_count, height_gap) = match repeat_y {
            Repeat | Round => {
                let (origin_rect, extend_height, count) = if (background_clip, background_origin)
                    == (BoxModelBox::BorderBox, BoxModelBox::PaddingBox)
                {
                    let extend_height =
                        extend(self.frame.border_width.y0 + bg_pos_y, bg_size.height);
                    let height = self.frame.border_box.height() + extend_height;
                    let count = (height / bg_size.height).ceil() as u32;

                    let origin_rect = Rect::from_origin_size(
                        Point::new(origin_rect.x0, self.frame.border_box.y0),
                        Size::new(origin_rect.width(), bg_size.height),
                    );

                    (origin_rect, extend_height, count)
                } else if (background_clip, background_origin)
                    == (BoxModelBox::BorderBox, BoxModelBox::ContentBox)
                {
                    let extend_height = extend(
                        self.frame.border_width.y0 + self.frame.padding_width.y0 + bg_pos_x,
                        bg_size.height,
                    );
                    let height = self.frame.border_box.height() + extend_height;
                    let count = (height / bg_size.height).ceil() as u32;

                    let origin_rect = Rect::from_origin_size(
                        Point::new(origin_rect.x0, self.frame.border_box.y0),
                        Size::new(origin_rect.width(), bg_size.height),
                    );

                    (origin_rect, extend_height, count)
                } else if (background_clip, background_origin)
                    == (BoxModelBox::PaddingBox, BoxModelBox::ContentBox)
                {
                    let extend_height =
                        extend(self.frame.padding_width.y0 + bg_pos_x, bg_size.height);
                    let height = self.frame.padding_box.height() + extend_height;
                    let count = (height / bg_size.height).ceil() as u32;

                    let origin_rect = Rect::from_origin_size(
                        Point::new(origin_rect.x0, self.frame.padding_box.y0),
                        Size::new(origin_rect.width(), bg_size.height),
                    );

                    (origin_rect, extend_height, count)
                } else {
                    let extend_height = extend(bg_pos_x, bg_size.height);
                    let height = origin_rect.height() + extend_height;
                    let count = (height / bg_size.height).ceil() as u32;
                    let origin_rect =
                        origin_rect.with_size(Size::new(origin_rect.width(), bg_size.height));

                    (origin_rect, extend_height, count)
                };

                let transform = transform.then_translate(Vec2 {
                    x: 0.0,
                    y: origin_rect.y0 - extend_height,
                });

                (origin_rect, transform, count, bg_size.height)
            }
            Space => {
                let (count, gap) =
                    compute_space_count_and_gap(origin_rect.height(), bg_size.height);

                let transform = transform.then_translate(Vec2 {
                    x: 0.0,
                    y: origin_rect.y0 + if count == 1 { bg_pos_y } else { 0.0 },
                });

                let origin_rect =
                    origin_rect.with_size(Size::new(origin_rect.width(), bg_size.height));

                (origin_rect, transform, count, gap)
            }
            NoRepeat => {
                let transform = transform.then_translate(Vec2 {
                    x: 0.0,
                    y: origin_rect.y0 + bg_pos_y,
                });

                let origin_rect =
                    origin_rect.with_size(Size::new(origin_rect.width(), bg_size.height));
                (origin_rect, transform, 1, 0.0)
            }
        };

        // FIXME: https://wpt.live/css/css-backgrounds/background-size/background-size-near-zero-gradient.html
        if width_count * height_count > 500 {
            return;
        }

        let origin_rect = Rect::new(0.0, 0.0, origin_rect.width(), origin_rect.height());
        let bounding_box = self.frame.border_box.bounding_box();
        let current_color = self.style.clone_color();

        let (gradient, gradient_transform) = to_peniko_gradient(
            gradient,
            origin_rect,
            bounding_box,
            self.scale,
            &current_color,
        );
        let brush = anyrender::Paint::Gradient(&gradient);

        for hc in 0..height_count {
            for wc in 0..width_count {
                let transform = transform.then_translate(Vec2 {
                    x: wc as f64 * width_gap,
                    y: hc as f64 * height_gap,
                });

                scene.fill(
                    peniko::Fill::NonZero,
                    transform,
                    brush.clone(),
                    gradient_transform,
                    &origin_rect,
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

fn compute_space_count_and_gap(bg_size: f64, size: f64) -> (u32, f64) {
    let modulo = bg_size % size;
    let count = (((bg_size - modulo) / size) as u32).max(1);
    let gap = if count > 1 {
        modulo / (count - 1) as f64
    } else {
        0.0
    } + size;

    (count, gap)
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
