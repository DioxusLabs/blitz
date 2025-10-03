// Copyright 2024 the Vello Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use crate::util;
use anyrender::PaintScene;
use kurbo::{Affine, BezPath};
use peniko::{BlendMode, BrushRef, Fill};
use usvg::{Node, Path};

pub(crate) fn render_group<S: PaintScene, F: FnMut(&mut S, &usvg::Node)>(
    scene: &mut S,
    group: &usvg::Group,
    transform: Affine,
    global_transform: Affine,
    error_handler: &mut F,
) {
    for node in group.children() {
        let transform = transform * util::to_affine(&node.abs_transform());
        match node {
            usvg::Node::Group(g) => {
                let alpha = g.opacity().get();
                let is_fully_opaque = alpha >= 1.0;
                let mix = util::to_mix(g.blend_mode(), is_fully_opaque);

                // Support clip-path with a single path
                let clip_path = g
                    .clip_path()
                    .and_then(|path| path.root().children().first());

                let did_push_layer = match clip_path {
                    // If there is a clip path, then push a layer that clips using it
                    Some(usvg::Node::Path(clip_path)) => {
                        let local_path = util::to_bez_path(clip_path);
                        scene.push_layer(
                            BlendMode {
                                mix,
                                compose: peniko::Compose::SrcOver,
                            },
                            alpha,
                            global_transform * transform,
                            &local_path,
                        );

                        true
                    }
                    // Else if there is blending to be done then push a layer with a rectangular clip
                    #[allow(deprecated)]
                    _ if mix != peniko::Mix::Clip => {
                        // Use bounding box as the clip path.
                        let bounding_box = g.layer_bounding_box();
                        let rect = kurbo::Rect::from_origin_size(
                            (bounding_box.x(), bounding_box.y()),
                            (bounding_box.width() as f64, bounding_box.height() as f64),
                        );
                        scene.push_layer(
                            BlendMode {
                                mix,
                                compose: peniko::Compose::SrcOver,
                            },
                            alpha,
                            global_transform * transform,
                            &rect,
                        );

                        true
                    }
                    // Else if there is no clip or blending then don't push a layer
                    _ => false,
                };

                render_group(scene, g, Affine::IDENTITY, global_transform, error_handler);

                if did_push_layer {
                    scene.pop_layer();
                }
            }
            usvg::Node::Path(path) => {
                if !path.is_visible() {
                    continue;
                }
                let local_path = util::to_bez_path(path);

                let transform = global_transform * transform;
                match path.paint_order() {
                    usvg::PaintOrder::FillAndStroke => {
                        fill(scene, error_handler, path, transform, &local_path, node);
                        stroke(scene, error_handler, path, transform, &local_path, node);
                    }
                    usvg::PaintOrder::StrokeAndFill => {
                        stroke(scene, error_handler, path, transform, &local_path, node);
                        fill(scene, error_handler, path, transform, &local_path, node);
                    }
                }
            }
            usvg::Node::Image(img) => {
                if !img.is_visible() {
                    continue;
                }
                match img.kind() {
                    usvg::ImageKind::JPEG(_)
                    | usvg::ImageKind::PNG(_)
                    | usvg::ImageKind::GIF(_)
                    | usvg::ImageKind::WEBP(_) => {
                        #[cfg(feature = "image")]
                        {
                            let Ok(decoded_image) = util::decode_raw_raster_image(img.kind())
                            else {
                                error_handler(scene, node);
                                continue;
                            };
                            let image = util::into_image(decoded_image);
                            let image_ts = global_transform * util::to_affine(&img.abs_transform());
                            scene.draw_image(image.as_ref(), image_ts);
                        }

                        #[cfg(not(feature = "image"))]
                        {
                            error_handler(scene, node);
                            continue;
                        }
                    }
                    usvg::ImageKind::SVG(svg) => {
                        render_group(
                            scene,
                            svg.root(),
                            transform,
                            global_transform,
                            error_handler,
                        );
                    }
                }
            }
            usvg::Node::Text(text) => {
                render_group(
                    scene,
                    text.flattened(),
                    transform,
                    global_transform,
                    error_handler,
                );
            }
        }
    }
}

fn fill<S: PaintScene, F: FnMut(&mut S, &usvg::Node)>(
    scene: &mut S,
    error_handler: &mut F,
    path: &Path,
    transform: Affine,
    local_path: &BezPath,
    node: &Node,
) {
    if let Some(fill) = &path.fill() {
        if let Some((brush, brush_transform)) = util::to_brush(fill.paint(), fill.opacity()) {
            scene.fill(
                match fill.rule() {
                    usvg::FillRule::NonZero => Fill::NonZero,
                    usvg::FillRule::EvenOdd => Fill::EvenOdd,
                },
                transform,
                BrushRef::from(&brush),
                Some(brush_transform),
                local_path,
            );
        } else {
            error_handler(scene, node);
        }
    }
}

fn stroke<S: PaintScene, F: FnMut(&mut S, &usvg::Node)>(
    scene: &mut S,
    error_handler: &mut F,
    path: &Path,
    transform: Affine,
    local_path: &BezPath,
    node: &Node,
) {
    if let Some(stroke) = &path.stroke() {
        if let Some((brush, brush_transform)) = util::to_brush(stroke.paint(), stroke.opacity()) {
            let conv_stroke = util::to_stroke(stroke);
            scene.stroke(
                &conv_stroke,
                transform,
                &brush,
                Some(brush_transform),
                local_path,
            );
        } else {
            error_handler(scene, node);
        }
    }
}
