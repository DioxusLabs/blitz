use super::{ElementCx, to_peniko_image};
use crate::color::{Color, ToColorColor};
use crate::layers::maybe_with_layer;
use blitz_dom::node::ImageData;
use color::DynamicColor;
use kurbo::{self, Affine, BezPath, Point, Rect, Shape, Size, Vec2};
use peniko::{self, Fill, Gradient};
use style::color::AbsoluteColor;
use style::{
    OwnedSlice,
    properties::{
        generated::longhands::{
            background_clip::single_value::computed_value::T as StyloBackgroundClip,
            background_origin::single_value::computed_value::T as StyloBackgroundOrigin,
        },
        style_structs::Background,
    },
    values::{
        computed::{
            Angle, AngleOrPercentage, BackgroundRepeat, CSSPixelLength, Gradient as StyloGradient,
            LengthPercentage, LineDirection, Percentage,
        },
        generics::{
            NonNegative,
            color::GenericColor,
            image::{
                EndingShape, GenericCircle, GenericEllipse, GenericEndingShape, GenericGradient,
                GenericGradientItem, GenericImage, GradientFlags, ShapeExtent,
            },
            position::GenericPosition,
        },
        specified::{
            background::BackgroundRepeatKeyword,
            percentage::ToPercentage,
            position::{HorizontalPositionKeyword, VerticalPositionKeyword},
        },
    },
};

type GradientItem<T> = GenericGradientItem<GenericColor<Percentage>, T>;
type LinearGradient<'a> = (
    &'a LineDirection,
    &'a [GradientItem<LengthPercentage>],
    GradientFlags,
);
type RadialGradient<'a> = (
    &'a EndingShape<NonNegative<CSSPixelLength>, NonNegative<LengthPercentage>>,
    &'a GenericPosition<LengthPercentage, LengthPercentage>,
    &'a OwnedSlice<GenericGradientItem<GenericColor<Percentage>, LengthPercentage>>,
    GradientFlags,
);
type ConicGradient<'a> = (
    &'a Angle,
    &'a GenericPosition<LengthPercentage, LengthPercentage>,
    &'a OwnedSlice<GenericGradientItem<GenericColor<Percentage>, AngleOrPercentage>>,
    GradientFlags,
);

impl ElementCx<'_> {
    pub(super) fn draw_background(&self, scene: &mut impl anyrender::Scene) {
        use GenericImage::*;
        use StyloBackgroundClip::*;

        let bg_styles = &self.style.get_background();

        let background_clip = get_cyclic(
            &bg_styles.background_clip.0,
            bg_styles.background_image.0.len() - 1,
        );
        let background_clip_path = match background_clip {
            BorderBox => self.frame.frame_border(),
            PaddingBox => self.frame.frame_padding(),
            ContentBox => self.frame.frame_content(),
        };

        // Draw background color (if any)
        self.draw_solid_frame(scene, &background_clip_path);

        for (idx, segment) in bg_styles.background_image.0.iter().enumerate().rev() {
            let background_clip = get_cyclic(&bg_styles.background_clip.0, idx);
            let background_clip_path = match background_clip {
                BorderBox => self.frame.frame_border(),
                PaddingBox => self.frame.frame_padding(),
                ContentBox => self.frame.frame_content(),
            };

            maybe_with_layer(
                scene,
                true,
                1.0,
                self.transform,
                &background_clip_path,
                |scene| {
                    match segment {
                        None => {
                            // Do nothing
                        }
                        Gradient(gradient) => {
                            self.draw_gradient_frame(scene, gradient, idx, *background_clip)
                        }
                        Url(_) => {
                            self.draw_raster_bg_image(scene, idx);
                            #[cfg(feature = "svg")]
                            self.draw_svg_bg_image(scene, idx);
                        }
                        PaintWorklet(_) => {
                            todo!("Implement background drawing for Image::PaintWorklet")
                        }
                        CrossFade(_) => todo!("Implement background drawing for Image::CrossFade"),
                        ImageSet(_) => todo!("Implement background drawing for Image::ImageSet"),
                    }
                },
            );
        }
    }

    fn draw_solid_frame(&self, scene: &mut impl anyrender::Scene, shape: &BezPath) {
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
    fn draw_svg_bg_image(&self, scene: &mut impl anyrender::Scene, idx: usize) {
        let bg_image = self.element.background_images.get(idx);

        let Some(Some(bg_image)) = bg_image.as_ref() else {
            return;
        };
        let ImageData::Svg(svg) = &bg_image.image else {
            return;
        };

        let bg_styles = &self.style.get_background();

        let frame_w = self.frame.padding_box.width() as f32;
        let frame_h = self.frame.padding_box.height() as f32;

        let svg_size = svg.size();
        let bg_size = compute_background_size(
            bg_styles,
            frame_w,
            frame_h,
            idx,
            BackgroundSizeComputeMode::Size(
                svg_size.width() / self.scale as f32,
                svg_size.height() / self.scale as f32,
            ),
            self.scale as f32,
        );
        let bg_size = bg_size * self.scale;

        let x_ratio = bg_size.width as f64 / svg_size.width() as f64;
        let y_ratio = bg_size.height as f64 / svg_size.height() as f64;

        let bg_pos = compute_background_position(
            bg_styles,
            idx,
            frame_w - bg_size.width as f32,
            frame_h - bg_size.height as f32,
        );

        let transform = Affine::translate((
            (self.pos.x * self.scale) + bg_pos.x,
            (self.pos.y * self.scale) + bg_pos.y,
        ))
        .pre_scale_non_uniform(x_ratio, y_ratio);

        anyrender_svg::append_tree(scene, svg, transform);
    }

    fn draw_raster_bg_image(&self, scene: &mut impl anyrender::Scene, idx: usize) {
        use BackgroundRepeatKeyword::*;

        let bg_image = self.element.background_images.get(idx);

        let Some(Some(bg_image)) = bg_image.as_ref() else {
            return;
        };
        let ImageData::Raster(image_data) = &bg_image.image else {
            return;
        };

        let bg_styles = &self.style.get_background();

        let background_origin = get_cyclic(&bg_styles.background_origin.0, idx);
        let origin_rect = match background_origin {
            StyloBackgroundOrigin::BorderBox => self.frame.border_box,
            StyloBackgroundOrigin::PaddingBox => self.frame.padding_box,
            StyloBackgroundOrigin::ContentBox => self.frame.content_box,
        };

        let image_width = image_data.width as f64;
        let image_height = image_data.height as f64;

        let (bg_pos, bg_size) = compute_background_position_and_background_size(
            bg_styles,
            origin_rect.width() / self.scale,
            origin_rect.height() / self.scale,
            idx,
            BackgroundSizeComputeMode::Size(image_width as f32, image_height as f32),
        );

        let bg_pos_x = bg_pos.x * self.scale;
        let bg_pos_y = bg_pos.y * self.scale;
        let bg_size = bg_size * self.scale;

        let x_ratio = bg_size.width / image_width;
        let y_ratio = bg_size.height / image_height;

        let BackgroundRepeat(repeat_x, repeat_y) = get_cyclic(&bg_styles.background_repeat.0, idx);

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
                        &to_peniko_image(image_data),
                        None,
                        &Rect::new(0.0, 0.0, origin_rect.width(), origin_rect.height()),
                    );
                }
            }
        } else {
            scene.fill(
                peniko::Fill::NonZero,
                transform,
                &to_peniko_image(image_data),
                None,
                &Rect::new(0.0, 0.0, origin_rect.width(), origin_rect.height()),
            );
        }
    }

    fn draw_gradient_frame(
        &self,
        scene: &mut impl anyrender::Scene,
        gradient: &StyloGradient,
        idx: usize,
        background_clip: StyloBackgroundClip,
    ) {
        use BackgroundRepeatKeyword::*;

        let bg_styles = &self.style.get_background();

        let background_origin = *get_cyclic(&bg_styles.background_origin.0, idx);
        let origin_rect = match background_origin {
            StyloBackgroundOrigin::BorderBox => self.frame.border_box,
            StyloBackgroundOrigin::PaddingBox => self.frame.padding_box,
            StyloBackgroundOrigin::ContentBox => self.frame.content_box,
        };

        let (bg_pos, bg_size) = compute_background_position_and_background_size(
            bg_styles,
            origin_rect.width() / self.scale,
            origin_rect.height() / self.scale,
            idx,
            BackgroundSizeComputeMode::Auto,
        );

        let bg_pos_x = bg_pos.x * self.scale;
        let bg_pos_y = bg_pos.y * self.scale;
        let bg_size = bg_size * self.scale;

        let BackgroundRepeat(repeat_x, repeat_y) = get_cyclic(&bg_styles.background_repeat.0, idx);

        let transform = self.transform;
        let (origin_rect, transform, width_count, width_gap) = match repeat_x {
            Repeat | Round => {
                let (origin_rect, extend_width, count) = if (background_clip, background_origin)
                    == (
                        StyloBackgroundClip::BorderBox,
                        StyloBackgroundOrigin::PaddingBox,
                    ) {
                    let extend_width =
                        extend(self.frame.border_left_width + bg_pos_x, bg_size.width);

                    let width = self.frame.border_box.width() + extend_width;
                    let count = (width / bg_size.width).ceil() as u32;

                    let origin_rect = Rect::from_origin_size(
                        Point::new(self.frame.border_box.x0, origin_rect.y0),
                        Size::new(bg_size.width, origin_rect.height()),
                    );

                    (origin_rect, extend_width, count)
                } else if (background_clip, background_origin)
                    == (
                        StyloBackgroundClip::BorderBox,
                        StyloBackgroundOrigin::ContentBox,
                    )
                {
                    let extend_width = extend(
                        self.frame.border_left_width + self.frame.padding_left_width + bg_pos_x,
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
                    == (
                        StyloBackgroundClip::PaddingBox,
                        StyloBackgroundOrigin::ContentBox,
                    )
                {
                    let extend_width =
                        extend(self.frame.padding_left_width + bg_pos_x, bg_size.width);
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
                    == (
                        StyloBackgroundClip::BorderBox,
                        StyloBackgroundOrigin::PaddingBox,
                    ) {
                    let extend_height =
                        extend(self.frame.border_top_width + bg_pos_y, bg_size.height);
                    let height = self.frame.border_box.height() + extend_height;
                    let count = (height / bg_size.height).ceil() as u32;

                    let origin_rect = Rect::from_origin_size(
                        Point::new(origin_rect.x0, self.frame.border_box.y0),
                        Size::new(origin_rect.width(), bg_size.height),
                    );

                    (origin_rect, extend_height, count)
                } else if (background_clip, background_origin)
                    == (
                        StyloBackgroundClip::BorderBox,
                        StyloBackgroundOrigin::ContentBox,
                    )
                {
                    let extend_height = extend(
                        self.frame.border_top_width + self.frame.padding_top_width + bg_pos_x,
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
                    == (
                        StyloBackgroundClip::PaddingBox,
                        StyloBackgroundOrigin::ContentBox,
                    )
                {
                    let extend_height =
                        extend(self.frame.padding_top_width + bg_pos_x, bg_size.height);
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

        let (gradient, gradient_transform) = match gradient {
            // https://developer.mozilla.org/en-US/docs/Web/CSS/gradient/linear-gradient
            GenericGradient::Linear {
                direction,
                items,
                flags,
                // compat_mode,
                ..
            } => self.linear_gradient((direction, items, *flags), origin_rect),
            GenericGradient::Radial {
                shape,
                position,
                items,
                flags,
                // compat_mode,
                ..
            } => self.radial_gradient((shape, position, items, *flags), origin_rect),
            GenericGradient::Conic {
                angle,
                position,
                items,
                flags,
                ..
            } => self.conic_gradient((angle, position, items, *flags), origin_rect),
        };
        let brush = peniko::BrushRef::Gradient(&gradient);

        for hc in 0..height_count {
            for wc in 0..width_count {
                let transform = transform.then_translate(Vec2 {
                    x: wc as f64 * width_gap,
                    y: hc as f64 * height_gap,
                });

                scene.fill(
                    peniko::Fill::NonZero,
                    transform,
                    brush,
                    gradient_transform,
                    &origin_rect,
                );
            }
        }
    }

    fn linear_gradient(
        &self,
        gradient: LinearGradient,
        rect: Rect,
    ) -> (peniko::Gradient, Option<Affine>) {
        let (direction, items, flags) = gradient;
        let bb = self.frame.border_box.bounding_box();
        let current_color = self.style.clone_color();

        let center = bb.center();
        let (start, end) = match direction {
            LineDirection::Angle(angle) => {
                let angle = -angle.radians64() + std::f64::consts::PI;
                let offset_length = rect.width() / 2.0 * angle.sin().abs()
                    + rect.height() / 2.0 * angle.cos().abs();
                let offset_vec = Vec2::new(angle.sin(), angle.cos()) * offset_length;
                (center - offset_vec, center + offset_vec)
            }
            LineDirection::Horizontal(horizontal) => {
                let start = Point::new(rect.x0, rect.y0 + rect.height() / 2.0);
                let end = Point::new(rect.x1, rect.y0 + rect.height() / 2.0);
                match horizontal {
                    HorizontalPositionKeyword::Right => (start, end),
                    HorizontalPositionKeyword::Left => (end, start),
                }
            }
            LineDirection::Vertical(vertical) => {
                let start = Point::new(rect.x0 + rect.width() / 2.0, rect.y0);
                let end = Point::new(rect.x0 + rect.width() / 2.0, rect.y1);
                match vertical {
                    VerticalPositionKeyword::Top => (end, start),
                    VerticalPositionKeyword::Bottom => (start, end),
                }
            }
            LineDirection::Corner(horizontal, vertical) => {
                let (start_x, end_x) = match horizontal {
                    HorizontalPositionKeyword::Right => (rect.x0, rect.x1),
                    HorizontalPositionKeyword::Left => (rect.x1, rect.x0),
                };
                let (start_y, end_y) = match vertical {
                    VerticalPositionKeyword::Top => (rect.y1, rect.y0),
                    VerticalPositionKeyword::Bottom => (rect.y0, rect.y1),
                };
                (Point::new(start_x, start_y), Point::new(end_x, end_y))
            }
        };

        let gradient_length = CSSPixelLength::new((start.distance(end) / self.scale) as f32);
        let repeating = flags.contains(GradientFlags::REPEATING);

        let mut gradient = peniko::Gradient::new_linear(start, end).with_extend(if repeating {
            peniko::Extend::Repeat
        } else {
            peniko::Extend::Pad
        });

        let (first_offset, last_offset) = Self::resolve_length_color_stops(
            current_color,
            items,
            gradient_length,
            &mut gradient,
            repeating,
        );
        if repeating && gradient.stops.len() > 1 {
            gradient.kind = peniko::GradientKind::Linear {
                start: start + (end - start) * first_offset as f64,
                end: end + (start - end) * (1.0 - last_offset) as f64,
            };
        }

        (gradient, None)
    }

    fn radial_gradient(
        &self,
        gradient: RadialGradient,
        rect: Rect,
    ) -> (peniko::Gradient, Option<Affine>) {
        let (shape, position, items, flags) = gradient;
        let repeating = flags.contains(GradientFlags::REPEATING);
        let current_color = self.style.clone_color();

        let mut gradient =
            peniko::Gradient::new_radial((0.0, 0.0), 1.0).with_extend(if repeating {
                peniko::Extend::Repeat
            } else {
                peniko::Extend::Pad
            });

        let (width_px, height_px) = (
            position
                .horizontal
                .resolve(CSSPixelLength::new(rect.width() as f32))
                .px() as f64,
            position
                .vertical
                .resolve(CSSPixelLength::new(rect.height() as f32))
                .px() as f64,
        );

        let gradient_scale: Option<Vec2> = match shape {
            GenericEndingShape::Circle(circle) => {
                let scale = match circle {
                    GenericCircle::Extent(extent) => match extent {
                        ShapeExtent::FarthestSide => width_px
                            .max(rect.width() - width_px)
                            .max(height_px.max(rect.height() - height_px)),
                        ShapeExtent::ClosestSide => width_px
                            .min(rect.width() - width_px)
                            .min(height_px.min(rect.height() - height_px)),
                        ShapeExtent::FarthestCorner => {
                            (width_px.max(rect.width() - width_px)
                                + height_px.max(rect.height() - height_px))
                                * 0.5_f64.sqrt()
                        }
                        ShapeExtent::ClosestCorner => {
                            (width_px.min(rect.width() - width_px)
                                + height_px.min(rect.height() - height_px))
                                * 0.5_f64.sqrt()
                        }
                        _ => 0.0,
                    },
                    GenericCircle::Radius(radius) => radius.0.px() as f64,
                };
                Some(Vec2::new(scale, scale))
            }
            GenericEndingShape::Ellipse(ellipse) => match ellipse {
                GenericEllipse::Extent(extent) => match extent {
                    ShapeExtent::FarthestCorner | ShapeExtent::FarthestSide => {
                        let mut scale = Vec2::new(
                            width_px.max(rect.width() - width_px),
                            height_px.max(rect.height() - height_px),
                        );
                        if *extent == ShapeExtent::FarthestCorner {
                            scale *= 2.0_f64.sqrt();
                        }
                        Some(scale)
                    }
                    ShapeExtent::ClosestCorner | ShapeExtent::ClosestSide => {
                        let mut scale = Vec2::new(
                            width_px.min(rect.width() - width_px),
                            height_px.min(rect.height() - height_px),
                        );
                        if *extent == ShapeExtent::ClosestCorner {
                            scale *= 2.0_f64.sqrt();
                        }
                        Some(scale)
                    }
                    _ => None,
                },
                GenericEllipse::Radii(x, y) => Some(Vec2::new(
                    x.0.resolve(CSSPixelLength::new(rect.width() as f32)).px() as f64,
                    y.0.resolve(CSSPixelLength::new(rect.height() as f32)).px() as f64,
                )),
            },
        };

        let gradient_transform = {
            // If the gradient has no valid scale, we don't need to calculate the color stops
            if let Some(gradient_scale) = gradient_scale {
                let (first_offset, last_offset) = Self::resolve_length_color_stops(
                    current_color,
                    items,
                    CSSPixelLength::new(gradient_scale.x as f32),
                    &mut gradient,
                    repeating,
                );
                let scale = if repeating && gradient.stops.len() >= 2 {
                    (last_offset - first_offset) as f64
                } else {
                    1.0
                };
                Some(
                    Affine::scale_non_uniform(gradient_scale.x * scale, gradient_scale.y * scale)
                        .then_translate(Self::get_translation(position, rect)),
                )
            } else {
                None
            }
        };

        (gradient, gradient_transform)
    }

    fn conic_gradient(
        &self,
        gradient: ConicGradient,
        rect: Rect,
    ) -> (peniko::Gradient, Option<Affine>) {
        let (angle, position, items, flags) = gradient;
        let current_color = self.style.clone_color();

        let repeating = flags.contains(GradientFlags::REPEATING);
        let mut gradient = peniko::Gradient::new_sweep((0.0, 0.0), 0.0, std::f32::consts::PI * 2.0)
            .with_extend(if repeating {
                peniko::Extend::Repeat
            } else {
                peniko::Extend::Pad
            });

        let (first_offset, last_offset) = Self::resolve_angle_color_stops(
            current_color,
            items,
            CSSPixelLength::new(1.0),
            &mut gradient,
            repeating,
        );
        if repeating && gradient.stops.len() >= 2 {
            gradient.kind = peniko::GradientKind::Sweep {
                center: Point::new(0.0, 0.0),
                start_angle: std::f32::consts::PI * 2.0 * first_offset,
                end_angle: std::f32::consts::PI * 2.0 * last_offset,
            };
        }

        let gradient_transform = Some(
            Affine::rotate(angle.radians() as f64 - std::f64::consts::PI / 2.0)
                .then_translate(Self::get_translation(position, rect)),
        );

        (gradient, gradient_transform)
    }

    #[inline]
    fn resolve_length_color_stops(
        current_color: AbsoluteColor,
        items: &[GradientItem<LengthPercentage>],
        gradient_length: CSSPixelLength,
        gradient: &mut Gradient,
        repeating: bool,
    ) -> (f32, f32) {
        Self::resolve_color_stops(
            current_color,
            items,
            gradient_length,
            gradient,
            repeating,
            |gradient_length: CSSPixelLength, position: &LengthPercentage| -> Option<f32> {
                position
                    .to_percentage_of(gradient_length)
                    .map(|percentage| percentage.to_percentage())
            },
        )
    }

    #[inline]
    fn resolve_color_stops<T>(
        current_color: AbsoluteColor,
        items: &[GradientItem<T>],
        gradient_length: CSSPixelLength,
        gradient: &mut Gradient,
        repeating: bool,
        item_resolver: impl Fn(CSSPixelLength, &T) -> Option<f32>,
    ) -> (f32, f32) {
        let mut hint: Option<f32> = None;

        for (idx, item) in items.iter().enumerate() {
            let (color, offset) = match item {
                GenericGradientItem::SimpleColorStop(color) => {
                    let step = 1.0 / (items.len() as f32 - 1.0);
                    (
                        color.resolve_to_absolute(&current_color).as_dynamic_color(),
                        step * idx as f32,
                    )
                }
                GenericGradientItem::ComplexColorStop { color, position } => {
                    let offset = item_resolver(gradient_length, position);
                    if let Some(offset) = offset {
                        (
                            color.resolve_to_absolute(&current_color).as_dynamic_color(),
                            offset,
                        )
                    } else {
                        continue;
                    }
                }
                GenericGradientItem::InterpolationHint(position) => {
                    hint = item_resolver(gradient_length, position);
                    continue;
                }
            };

            if idx == 0 && !repeating && offset != 0.0 {
                gradient
                    .stops
                    .push(peniko::ColorStop { color, offset: 0.0 });
            }

            match hint {
                None => gradient.stops.push(peniko::ColorStop { color, offset }),
                Some(hint) => {
                    let &last_stop = gradient.stops.last().unwrap();

                    if hint <= last_stop.offset {
                        // Upstream code has a bug here, so we're going to do something different
                        match gradient.stops.len() {
                            0 => (),
                            1 => {
                                gradient.stops.pop();
                            }
                            _ => {
                                let prev_stop = gradient.stops[gradient.stops.len() - 2];
                                if prev_stop.offset == hint {
                                    gradient.stops.pop();
                                }
                            }
                        }
                        gradient.stops.push(peniko::ColorStop {
                            color,
                            offset: hint,
                        });
                    } else if hint >= offset {
                        gradient.stops.push(peniko::ColorStop {
                            color: last_stop.color,
                            offset: hint,
                        });
                        gradient.stops.push(peniko::ColorStop {
                            color,
                            offset: last_stop.offset,
                        });
                    } else if hint == (last_stop.offset + offset) / 2.0 {
                        gradient.stops.push(peniko::ColorStop { color, offset });
                    } else {
                        let mid_point = (hint - last_stop.offset) / (offset - last_stop.offset);
                        let mut interpolate_stop = |cur_offset: f32| {
                            let relative_offset =
                                (cur_offset - last_stop.offset) / (offset - last_stop.offset);
                            let multiplier = relative_offset.powf(0.5f32.log(mid_point));
                            let [last_r, last_g, last_b, last_a] = last_stop.color.components;
                            let [r, g, b, a] = color.components;

                            let color = Color::new([
                                (last_r + multiplier * (r - last_r)),
                                (last_g + multiplier * (g - last_g)),
                                (last_b + multiplier * (b - last_b)),
                                (last_a + multiplier * (a - last_a)),
                            ]);
                            gradient.stops.push(peniko::ColorStop {
                                color: DynamicColor::from_alpha_color(color),
                                offset: cur_offset,
                            });
                        };
                        if mid_point > 0.5 {
                            for i in 0..7 {
                                interpolate_stop(
                                    last_stop.offset
                                        + (hint - last_stop.offset) * (7.0 + i as f32) / 13.0,
                                );
                            }
                            interpolate_stop(hint + (offset - hint) / 3.0);
                            interpolate_stop(hint + (offset - hint) * 2.0 / 3.0);
                        } else {
                            interpolate_stop(last_stop.offset + (hint - last_stop.offset) / 3.0);
                            interpolate_stop(
                                last_stop.offset + (hint - last_stop.offset) * 2.0 / 3.0,
                            );
                            for i in 0..7 {
                                interpolate_stop(hint + (offset - hint) * (i as f32) / 13.0);
                            }
                        }
                        gradient.stops.push(peniko::ColorStop { color, offset });
                    }
                }
            }
        }

        // Post-process the stops for repeating gradients
        if repeating && gradient.stops.len() > 1 {
            let first_offset = gradient.stops.first().unwrap().offset;
            let last_offset = gradient.stops.last().unwrap().offset;
            if first_offset != 0.0 || last_offset != 1.0 {
                let scale_inv = 1e-7_f32.max(1.0 / (last_offset - first_offset));
                for stop in &mut *gradient.stops {
                    stop.offset = (stop.offset - first_offset) * scale_inv;
                }
            }
            (first_offset, last_offset)
        } else {
            (0.0, 1.0)
        }
    }

    #[inline]
    fn resolve_angle_color_stops(
        current_color: AbsoluteColor,
        items: &[GradientItem<AngleOrPercentage>],
        gradient_length: CSSPixelLength,
        gradient: &mut Gradient,
        repeating: bool,
    ) -> (f32, f32) {
        Self::resolve_color_stops(
            current_color,
            items,
            gradient_length,
            gradient,
            repeating,
            |_gradient_length: CSSPixelLength, position: &AngleOrPercentage| -> Option<f32> {
                match position {
                    AngleOrPercentage::Angle(angle) => {
                        Some(angle.radians() / (std::f64::consts::PI * 2.0) as f32)
                    }
                    AngleOrPercentage::Percentage(percentage) => Some(percentage.to_percentage()),
                }
            },
        )
    }

    #[inline]
    fn get_translation(
        position: &GenericPosition<LengthPercentage, LengthPercentage>,
        rect: Rect,
    ) -> Vec2 {
        Vec2::new(
            rect.x0
                + position
                    .horizontal
                    .resolve(CSSPixelLength::new(rect.width() as f32))
                    .px() as f64,
            rect.y0
                + position
                    .vertical
                    .resolve(CSSPixelLength::new(rect.height() as f32))
                    .px() as f64,
        )
    }
}

fn compute_background_position_and_background_size(
    background: &Background,
    container_w: f64,
    container_h: f64,
    bg_idx: usize,
    size_mode: BackgroundSizeComputeMode,
) -> (Point, Size) {
    use BackgroundRepeatKeyword::*;

    let bg_size = compute_background_size(
        background,
        container_w as f32,
        container_h as f32,
        bg_idx,
        size_mode,
        1.0,
    );

    let bg_pos = compute_background_position(
        background,
        bg_idx,
        (container_w - bg_size.width) as f32,
        (container_h - bg_size.height) as f32,
    );

    let BackgroundRepeat(repeat_x, repeat_y) = get_cyclic(&background.background_repeat.0, bg_idx);

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
fn compute_background_position(
    background: &Background,
    bg_idx: usize,
    width: f32,
    height: f32,
) -> Point {
    use style::values::computed::Length;

    let bg_pos_x = get_cyclic(&background.background_position_x.0, bg_idx)
        .resolve(Length::new(width))
        .px() as f64;
    let bg_pos_y = get_cyclic(&background.background_position_y.0, bg_idx)
        .resolve(Length::new(height))
        .px() as f64;

    Point::new(bg_pos_x, bg_pos_y)
}

fn compute_background_size(
    background: &Background,
    container_w: f32,
    container_h: f32,
    bg_idx: usize,
    mode: BackgroundSizeComputeMode,
    scale: f32,
) -> kurbo::Size {
    use style::values::computed::{BackgroundSize, Length};
    use style::values::generics::length::GenericLengthPercentageOrAuto as Lpa;

    let bg_size = get_cyclic(&background.background_size.0, bg_idx);

    let (width, height): (f32, f32) = match bg_size {
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
                    BackgroundSizeComputeMode::Size(bg_w, bg_h) => (bg_w * scale, bg_h * scale),
                },
            }
        }
        BackgroundSize::Cover => match mode {
            BackgroundSizeComputeMode::Auto => (container_w, container_h),
            BackgroundSizeComputeMode::Size(bg_w, bg_h) => {
                let x_ratio = container_w / bg_w;
                let y_ratio = container_h / bg_h;

                let ratio = if x_ratio < 1.0 || y_ratio < 1.0 {
                    x_ratio.min(y_ratio)
                } else {
                    x_ratio.max(y_ratio)
                };

                (bg_w * ratio, bg_h * ratio)
            }
        },
        BackgroundSize::Contain => match mode {
            BackgroundSizeComputeMode::Auto => (container_w, container_h),
            BackgroundSizeComputeMode::Size(bg_w, bg_h) => {
                let x_ratio = container_w / bg_w;
                let y_ratio = container_h / bg_h;

                let ratio = if x_ratio < 1.0 || y_ratio < 1.0 {
                    x_ratio.max(y_ratio)
                } else {
                    x_ratio.min(y_ratio)
                };

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
fn get_cyclic<T>(values: &[T], layer_index: usize) -> &T {
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
