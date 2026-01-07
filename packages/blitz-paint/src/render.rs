mod background;
mod box_shadow;
mod form_controls;

use std::any::Any;
use std::collections::HashMap;

use super::kurbo_css::{CssBox, Edge};
use crate::SELECTION_COLOR;
use crate::color::{Color, ToColorColor};
use crate::debug_overlay::render_debug_overlay;
use crate::kurbo_css::NonUniformRoundedRectRadii;
use crate::layers::LayerManager;
use crate::sizing::compute_object_fit;
use anyrender::{CustomPaint, Paint, PaintScene};
use blitz_dom::node::{
    ListItemLayout, ListItemLayoutPosition, Marker, NodeData, RasterImageData, SpecialElementData,
    TextInputData, TextNodeData,
};
use blitz_dom::{BaseDocument, ElementData, Node, local_name};
use blitz_traits::devtools::DevtoolSettings;

use euclid::Transform3D;
use style::values::computed::BorderCornerRadius;
use style::{
    computed_values::border_collapse::T as BorderCollapse,
    dom::TElement,
    properties::{
        ComputedValues, generated::longhands::visibility::computed_value::T as StyloVisibility,
        style_structs::Font,
    },
    values::{
        computed::{CSSPixelLength, Overflow},
        specified::{BorderStyle, OutlineStyle, image::ImageRendering},
    },
};

use kurbo::{self, Affine, Insets, Point, Rect, Stroke, Vec2};
use peniko::{self, Fill, ImageData, ImageSampler};
use style::values::generics::color::GenericColor;
use taffy::Layout;

/// A short-lived struct which holds a bunch of parameters for rendering a scene so
/// that we don't have to pass them down as parameters
pub struct BlitzDomPainter<'dom> {
    /// Input parameters (read only) for generating the Scene
    pub(crate) dom: &'dom BaseDocument,
    pub(crate) scale: f64,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) initial_x: f64,
    pub(crate) initial_y: f64,
    pub(crate) layer_manager: LayerManager,
    /// Cached selection ranges for O(1) lookup: node_id -> (start_offset, end_offset)
    pub(crate) selection_ranges: HashMap<usize, (usize, usize)>,
}

impl<'dom> BlitzDomPainter<'dom> {
    /// Create a new BlitzDomPainter for the given document
    pub fn new(
        dom: &'dom BaseDocument,
        scale: f64,
        width: u32,
        height: u32,
        initial_x: f64,
        initial_y: f64,
    ) -> Self {
        let selection_ranges: HashMap<usize, (usize, usize)> = dom
            .get_text_selection_ranges()
            .into_iter()
            .map(|(node_id, start, end)| (node_id, (start, end)))
            .collect();

        let layer_manager = LayerManager::default();

        Self {
            dom,
            scale,
            width,
            height,
            initial_x,
            initial_y,
            layer_manager,
            selection_ranges,
        }
    }

    fn node_position(&self, node: usize, location: Point) -> (Layout, Point) {
        let layout = self.layout(node);
        let pos = location + Vec2::new(layout.location.x as f64, layout.location.y as f64);
        (layout, pos)
    }

    fn layout(&self, child: usize) -> Layout {
        // self.dom.as_ref().tree()[child].unrounded_layout
        self.dom.as_ref().tree()[child].final_layout
    }

    /// Draw the current tree to current render surface
    /// Eventually we'll want the surface itself to be passed into the render function, along with things like the viewport
    ///
    /// This assumes styles are resolved and layout is complete.
    /// Make sure you do those before trying to render
    pub fn paint_scene(&self, scene: &mut impl PaintScene) {
        // Simply render the document (the root element (note that this is not the same as the root node)))
        // scene.reset();
        let viewport_scroll = self.dom.as_ref().viewport_scroll();

        let root_element = self.dom.as_ref().root_element();
        let root_id = root_element.id;
        let bg_width = (self.width as f32).max(root_element.final_layout.size.width);
        let bg_height = (self.height as f32).max(root_element.final_layout.size.height);

        let background_color = {
            let html_color = root_element
                .primary_styles()
                .map(|s| s.clone_background_color())
                .unwrap_or(GenericColor::TRANSPARENT_BLACK);
            if html_color == GenericColor::TRANSPARENT_BLACK {
                root_element
                    .children
                    .iter()
                    .find_map(|id| {
                        self.dom
                            .as_ref()
                            .get_node(*id)
                            .filter(|node| node.data.is_element_with_tag_name(&local_name!("body")))
                    })
                    .and_then(|body| body.primary_styles())
                    .map(|style| {
                        let current_color = style.clone_color();
                        style
                            .clone_background_color()
                            .resolve_to_absolute(&current_color)
                    })
            } else {
                let current_color = root_element.primary_styles().unwrap().clone_color();
                Some(html_color.resolve_to_absolute(&current_color))
            }
        };

        if let Some(bg_color) = background_color {
            let bg_color = bg_color.as_srgb_color();
            let rect = Rect::from_origin_size((0.0, 0.0), (bg_width as f64, bg_height as f64));
            scene.fill(Fill::NonZero, Affine::IDENTITY, bg_color, None, &rect);
        }

        self.render_element(
            scene,
            root_id,
            Point {
                x: self.initial_x - viewport_scroll.x,
                y: self.initial_y - viewport_scroll.y,
            },
        );

        // Render debug overlay
        if self.dom.devtools().highlight_hover {
            if let Some(node_id) = self.dom.as_ref().get_hover_node_id() {
                render_debug_overlay(
                    scene,
                    self.dom,
                    node_id,
                    self.scale,
                    self.initial_x,
                    self.initial_y,
                );
            }
        }
    }

    /// Renders a node, but is guaranteed that the node is an element
    /// This is because the font_size is calculated from layout resolution and all text is rendered directly here, instead
    /// of a separate text stroking phase.
    ///
    /// In Blitz, text styling gets its attributes from its container element/resolved styles
    /// In other libraries, text gets its attributes from a `text` element - this is not how HTML works.
    ///
    /// Approaching rendering this way guarantees we have all the styles we need when rendering text with not having
    /// to traverse back to the parent for its styles, or needing to pass down styles
    fn render_element(&self, scene: &mut impl PaintScene, node_id: usize, location: Point) {
        let node = &self.dom.as_ref().tree()[node_id];

        // Early return if the element is hidden
        if matches!(node.style.display, taffy::Display::None) {
            return;
        }

        // Only draw elements with a style
        let Some(styles) = node.primary_styles() else {
            return;
        };

        // Hide inputs with type=hidden
        // Implemented here rather than using the style engine for performance reasons
        if node.local_name() == "input" && node.attr(local_name!("type")) == Some("hidden") {
            return;
        }

        // Hide elements with a visibility style other than visible
        if styles.get_inherited_box().visibility != StyloVisibility::Visible {
            return;
        }

        // We can't fully support opacity yet, but we can hide elements with opacity 0
        let opacity = styles.get_effects().opacity;
        if opacity == 0.0 {
            return;
        }
        let has_opacity = opacity < 1.0;

        // TODO: account for overflow_x vs overflow_y
        let overflow_x = styles.get_box().overflow_x;
        let overflow_y = styles.get_box().overflow_y;
        let is_image = node
            .element_data()
            .and_then(|e| e.raster_image_data())
            .is_some();
        let is_sub_doc = node
            .element_data()
            .and_then(|el| el.sub_doc_data())
            .is_some();
        let is_text_input = node
            .element_data()
            .and_then(|el| el.text_input_data())
            .is_some();
        let should_clip = is_image
            || is_sub_doc
            || is_text_input
            || !matches!(overflow_x, Overflow::Visible)
            || !matches!(overflow_y, Overflow::Visible);

        // Apply padding/border offset to inline root
        let (layout, box_position) = self.node_position(node_id, location);
        let taffy::Layout {
            size,
            border,
            padding,
            content_size,
            ..
        } = node.final_layout;
        let scaled_pb = (padding + border).map(f64::from);
        let content_position = kurbo::Point {
            x: scaled_pb.left,
            y: scaled_pb.top,
        };
        let content_box_size = kurbo::Size {
            width: (size.width as f64 - scaled_pb.left - scaled_pb.right) * self.scale,
            height: (size.height as f64 - scaled_pb.top - scaled_pb.bottom) * self.scale,
        };

        // Don't render things that are out of view
        let scaled_y = (box_position.y - self.initial_y) * self.scale;
        let scaled_content_height = content_size.height.max(size.height) as f64 * self.scale;
        if scaled_y > self.height as f64 || scaled_y + scaled_content_height < 0.0 {
            return;
        }

        // Optimise zero-area (/very small area) clips by not rendering at all
        let clip_area = content_box_size.width * content_box_size.height;
        if should_clip && clip_area < 0.01 {
            return;
        }

        let mut cx = self.element_cx(node, layout, box_position);

        cx.draw_outline(scene);
        cx.draw_outset_box_shadow(scene);

        // Opacity layer if box has opacity. Clipped to border-box as it needs to include
        // the background and borders.
        self.layer_manager.maybe_with_layer(
            scene,
            has_opacity,
            opacity,
            cx.transform,
            &cx.frame.border_box_path(),
            |scene| {
                cx.draw_background(scene);
                cx.draw_inset_box_shadow(scene);
                cx.draw_table_row_backgrounds(scene);
                cx.draw_table_borders(scene);
                cx.draw_border(scene);
                cx.stroke_devtools(scene);

                // TODO: allow layers with opacity to be unclipped (overflow: visible)
                let clip = if is_text_input {
                    &cx.frame.content_box_path()
                } else {
                    &cx.frame.padding_box_path()
                };

                // Clip layer if box requires clipping. Opacity set to 1.0
                self.layer_manager.maybe_with_layer(
                    scene,
                    should_clip,
                    1.0, // opacity
                    cx.transform,
                    clip,
                    |scene| {
                        // Now that background has been drawn, offset pos and cx in order to draw our contents scrolled
                        let content_position = Point {
                            x: content_position.x - node.scroll_offset.x,
                            y: content_position.y - node.scroll_offset.y,
                        };
                        cx.pos = Point {
                            x: cx.pos.x - node.scroll_offset.x,
                            y: cx.pos.y - node.scroll_offset.y,
                        };
                        cx.transform = cx.transform.then_translate(Vec2 {
                            x: -node.scroll_offset.x,
                            y: -node.scroll_offset.y,
                        });
                        cx.draw_image(scene);
                        #[cfg(feature = "svg")]
                        cx.draw_svg(scene);
                        cx.draw_canvas(scene);
                        cx.draw_sub_document(scene);
                        cx.draw_input(scene);
                        cx.draw_text_input_text(scene, content_position);
                        cx.draw_inline_layout(scene, content_position);
                        cx.draw_marker(scene, content_position);
                        cx.draw_children(scene);
                    },
                );
            },
        );
    }

    fn render_node(&self, scene: &mut impl PaintScene, node_id: usize, location: Point) {
        let node = &self.dom.as_ref().tree()[node_id];

        match &node.data {
            NodeData::Element(_) | NodeData::AnonymousBlock(_) => {
                self.render_element(scene, node_id, location)
            }
            NodeData::Text(TextNodeData { .. }) => {
                // Text nodes should never be rendered directly
                // (they should always be rendered as part of an inline layout)
                // unreachable!()
            }
            NodeData::Document => {}
            // NodeData::Doctype => {}
            NodeData::Comment => {} // NodeData::ProcessingInstruction { .. } => {}
        }
    }

    fn element_cx<'w>(
        &'w self,
        node: &'w Node,
        layout: Layout,
        box_position: Point,
    ) -> ElementCx<'w> {
        let style = node
            .stylo_element_data
            .borrow()
            .as_ref()
            .map(|element_data| element_data.styles.primary().clone())
            .unwrap_or(
                ComputedValues::initial_values_with_font_override(Font::initial_values()).to_arc(),
            );

        let scale = self.scale;

        // todo: maybe cache this so we don't need to constantly be figuring it out
        // It is quite a bit of math to calculate during render/traverse
        // Also! we can cache the bezpaths themselves, saving us a bunch of work
        let frame = create_css_rect(&style, &layout, scale);

        // the bezpaths for every element are (potentially) cached (not yet, tbd)
        // By performing the transform, we prevent the cache from becoming invalid when the page shifts around
        let mut transform = Affine::translate(box_position.to_vec2() * scale);

        // Reference box for resolve percentage transforms
        let reference_box = euclid::Rect::new(
            euclid::Point2D::new(CSSPixelLength::new(0.0), CSSPixelLength::new(0.0)),
            euclid::Size2D::new(
                CSSPixelLength::new((frame.border_box.width() / scale) as f32),
                CSSPixelLength::new((frame.border_box.height() / scale) as f32),
            ),
        );

        // Apply CSS transform property (where transforms are 2d)
        //
        // TODO: Handle hit testing correctly for transformed nodes
        // TODO: Implement nested transforms
        let (t, has_3d) = &style
            .get_box()
            .transform
            .to_transform_3d_matrix(Some(&reference_box))
            .unwrap_or((Transform3D::default(), false));
        if !has_3d {
            // See: https://drafts.csswg.org/css-transforms-2/#two-dimensional-subset
            // And https://docs.rs/kurbo/latest/kurbo/struct.Affine.html#method.new
            let kurbo_transform = Affine::new(
                [
                    t.m11,
                    t.m12,
                    t.m21,
                    t.m22,
                    // Scale the translation but not the scale or skew
                    t.m41 * scale as f32,
                    t.m42 * scale as f32,
                ]
                .map(|v| v as f64),
            );

            // Apply the transform origin by:
            //   - Translating by the origin offset
            //   - Applying our transform
            //   - Translating by the inverse of the origin offset
            let transform_origin = &style.get_box().transform_origin;
            let origin_translation = Affine::translate(Vec2 {
                x: transform_origin
                    .horizontal
                    .resolve(CSSPixelLength::new(frame.border_box.width() as f32))
                    .px() as f64,
                y: transform_origin
                    .vertical
                    .resolve(CSSPixelLength::new(frame.border_box.height() as f32))
                    .px() as f64,
            });
            let kurbo_transform =
                origin_translation * kurbo_transform * origin_translation.inverse();

            transform *= kurbo_transform;
        }

        let element = node.element_data().unwrap();

        ElementCx {
            context: self,
            frame,
            scale,
            style,
            pos: box_position,
            node,
            element,
            transform,
            #[cfg(feature = "svg")]
            svg: element.svg_data(),
            text_input: element.text_input_data(),
            list_item: element.list_item_data.as_deref(),
            devtools: self.dom.devtools(),
        }
    }
}

fn to_image_quality(image_rendering: ImageRendering) -> peniko::ImageQuality {
    match image_rendering {
        ImageRendering::Auto => peniko::ImageQuality::Medium,
        ImageRendering::CrispEdges => peniko::ImageQuality::Low,
        ImageRendering::Pixelated => peniko::ImageQuality::Low,
    }
}

/// Ensure that the `resized_image` field has a correctly sized image
fn to_peniko_image(image: &RasterImageData, quality: peniko::ImageQuality) -> peniko::ImageBrush {
    peniko::ImageBrush {
        image: ImageData {
            data: image.data.clone(),
            format: peniko::ImageFormat::Rgba8,
            width: image.width,
            height: image.height,
            alpha_type: peniko::ImageAlphaType::Alpha,
        },
        sampler: ImageSampler {
            x_extend: peniko::Extend::Repeat,
            y_extend: peniko::Extend::Repeat,
            quality,
            alpha: 1.0,
        },
    }
}

/// A context of loaded and hot data to draw the element from
struct ElementCx<'a> {
    context: &'a BlitzDomPainter<'a>,
    frame: CssBox,
    style: style::servo_arc::Arc<ComputedValues>,
    pos: Point,
    scale: f64,
    node: &'a Node,
    element: &'a ElementData,
    transform: Affine,
    #[cfg(feature = "svg")]
    svg: Option<&'a usvg::Tree>,
    text_input: Option<&'a TextInputData>,
    list_item: Option<&'a ListItemLayout>,
    devtools: &'a DevtoolSettings,
}

/// Converts parley BoundingBox into peniko Rect
fn convert_rect(rect: &parley::BoundingBox) -> kurbo::Rect {
    peniko::kurbo::Rect::new(rect.x0, rect.y0, rect.x1, rect.y1)
}

impl ElementCx<'_> {
    fn draw_inline_layout(&self, scene: &mut impl PaintScene, pos: Point) {
        if self.node.flags.is_inline_root() {
            let text_layout = self.element
                .inline_layout_data
                .as_ref()
                .unwrap_or_else(|| {
                    panic!("Tried to render node marked as inline root that does not have an inline layout: {:?}", self.node);
                });

            let transform =
                Affine::translate((pos.x * self.scale, pos.y * self.scale)) * self.transform;

            // Render text selection highlight (if any) using cached selection ranges
            if let Some(&(sel_start, sel_end)) = self.context.selection_ranges.get(&self.node.id) {
                crate::text::draw_text_selection(
                    scene,
                    &text_layout.layout,
                    transform,
                    sel_start,
                    sel_end,
                );
            }

            // Render text
            crate::text::stroke_text(
                scene,
                text_layout.layout.lines(),
                self.context.dom,
                transform,
            );
        }
    }

    fn draw_text_input_text(&self, scene: &mut impl PaintScene, pos: Point) {
        // Render the text in text inputs
        if let Some(input_data) = self.text_input {
            // For single-line inputs, add an offset to vertically center the text input layout
            // within the content box of it's node.
            let y_offset = self.node.text_input_v_centering_offset(self.scale);
            let pos = Point {
                x: pos.x,
                y: pos.y + y_offset,
            };

            let transform =
                Affine::translate((pos.x * self.scale, pos.y * self.scale)) * self.transform;

            if self.node.is_focussed() {
                // Render selection/caret
                for (rect, _line_idx) in input_data.editor.selection_geometry().iter() {
                    scene.fill(
                        Fill::NonZero,
                        transform,
                        SELECTION_COLOR,
                        None,
                        &convert_rect(rect),
                    );
                }
                if let Some(cursor) = input_data.editor.cursor_geometry(1.5) {
                    // TODO: Use the `caret-color` attribute here if present.
                    let color = self.style.get_inherited_text().color;

                    scene.fill(
                        Fill::NonZero,
                        transform,
                        color.as_srgb_color(),
                        None,
                        &convert_rect(&cursor),
                    );
                };
            }

            // Render text
            crate::text::stroke_text(
                scene,
                input_data.editor.try_layout().unwrap().lines(),
                self.context.dom,
                transform,
            );
        }
    }

    fn draw_marker(&self, scene: &mut impl PaintScene, pos: Point) {
        if let Some(ListItemLayout {
            marker,
            position: ListItemLayoutPosition::Outside(layout),
        }) = self.list_item
        {
            // Right align and pad the bullet when rendering outside
            let x_padding = match marker {
                Marker::Char(_) => 8.0,
                Marker::String(_) => 0.0,
            };
            let x_offset = -(layout.full_width() / layout.scale() + x_padding);

            // Align the marker with the baseline of the first line of text in the list item
            let y_offset = if let Some(first_text_line) = &self
                .element
                .inline_layout_data
                .as_ref()
                .and_then(|text_layout| text_layout.layout.lines().next())
            {
                (first_text_line.metrics().baseline
                    - layout.lines().next().unwrap().metrics().baseline)
                    / layout.scale()
            } else {
                0.0
            };

            let pos = Point {
                x: pos.x + x_offset as f64,
                y: pos.y + y_offset as f64,
            };

            let transform =
                Affine::translate((pos.x * self.scale, pos.y * self.scale)) * self.transform;

            crate::text::stroke_text(scene, layout.lines(), self.context.dom, transform);
        }
    }

    fn draw_children(&self, scene: &mut impl PaintScene) {
        // Negative z_index hoisted nodes
        if let Some(hoisted) = &self.node.stacking_context {
            for hoisted_child in hoisted.neg_z_hoisted_children() {
                let pos = kurbo::Point {
                    x: self.pos.x + hoisted_child.position.x as f64,
                    y: self.pos.y + hoisted_child.position.y as f64,
                };
                self.render_node(scene, hoisted_child.node_id, pos);
            }
        }

        // Regular children
        if let Some(children) = &*self.node.paint_children.borrow() {
            for child_id in children {
                self.render_node(scene, *child_id, self.pos);
            }
        }

        // Positive z_index hoisted nodes
        if let Some(hoisted) = &self.node.stacking_context {
            for hoisted_child in hoisted.pos_z_hoisted_children() {
                let pos = kurbo::Point {
                    x: self.pos.x + hoisted_child.position.x as f64,
                    y: self.pos.y + hoisted_child.position.y as f64,
                };
                self.render_node(scene, hoisted_child.node_id, pos);
            }
        }
    }

    #[cfg(feature = "svg")]
    fn draw_svg(&self, scene: &mut impl PaintScene) {
        use style::properties::generated::longhands::object_fit::computed_value::T as ObjectFit;

        let Some(svg) = self.svg else {
            return;
        };

        let width = self.frame.content_box.width() as u32;
        let height = self.frame.content_box.height() as u32;
        let svg_size = svg.size();

        let x = self.frame.content_box.origin().x;
        let y = self.frame.content_box.origin().y;

        // let object_fit = self.style.clone_object_fit();
        let object_position = self.style.clone_object_position();

        // Apply object-fit algorithm
        let container_size = taffy::Size {
            width: width as f32,
            height: height as f32,
        };
        let object_size = taffy::Size {
            width: svg_size.width(),
            height: svg_size.height(),
        };
        let paint_size = compute_object_fit(container_size, Some(object_size), ObjectFit::Contain);

        // Compute object-position
        let x_offset = object_position.horizontal.resolve(
            CSSPixelLength::new(container_size.width - paint_size.width) / self.scale as f32,
        ) * self.scale as f32;
        let y_offset = object_position.vertical.resolve(
            CSSPixelLength::new(container_size.height - paint_size.height) / self.scale as f32,
        ) * self.scale as f32;
        let x = x + x_offset.px() as f64;
        let y = y + y_offset.px() as f64;

        let x_scale = paint_size.width as f64 / object_size.width as f64;
        let y_scale = paint_size.height as f64 / object_size.height as f64;

        let transform = self
            .transform
            .pre_scale_non_uniform(x_scale, y_scale)
            .then_translate(Vec2 { x, y });

        anyrender_svg::render_svg_tree(scene, svg, transform);
    }

    fn draw_image(&self, scene: &mut impl PaintScene) {
        if let Some(image) = self.element.raster_image_data() {
            let width = self.frame.content_box.width() as u32;
            let height = self.frame.content_box.height() as u32;
            let x = self.frame.content_box.origin().x;
            let y = self.frame.content_box.origin().y;

            let object_fit = self.style.clone_object_fit();
            let object_position = self.style.clone_object_position();
            let image_rendering = self.style.clone_image_rendering();
            let quality = to_image_quality(image_rendering);

            // Apply object-fit algorithm
            let container_size = taffy::Size {
                width: width as f32,
                height: height as f32,
            };
            let object_size = taffy::Size {
                width: image.width as f32,
                height: image.height as f32,
            };
            let paint_size = compute_object_fit(container_size, Some(object_size), object_fit);

            // Compute object-position
            let x_offset = object_position.horizontal.resolve(
                CSSPixelLength::new(container_size.width - paint_size.width) / self.scale as f32,
            ) * self.scale as f32;
            let y_offset = object_position.vertical.resolve(
                CSSPixelLength::new(container_size.height - paint_size.height) / self.scale as f32,
            ) * self.scale as f32;
            let x = x + x_offset.px() as f64;
            let y = y + y_offset.px() as f64;

            let x_scale = paint_size.width as f64 / object_size.width as f64;
            let y_scale = paint_size.height as f64 / object_size.height as f64;
            let transform = self
                .transform
                .pre_translate(Vec2 { x, y })
                .pre_scale_non_uniform(x_scale, y_scale);

            scene.draw_image(to_peniko_image(image, quality).as_ref(), transform);
        }
    }

    fn draw_canvas(&self, scene: &mut impl PaintScene) {
        if let Some(custom_paint_source) = self.element.canvas_data() {
            let width = self.frame.content_box.width() as u32;
            let height = self.frame.content_box.height() as u32;
            let x = self.frame.content_box.origin().x;
            let y = self.frame.content_box.origin().y;

            let transform = self.transform.then_translate(Vec2 { x, y });

            scene.fill(
                Fill::NonZero,
                transform,
                // TODO: replace `Arc<dyn Any>` with `CustomPaint` in API?
                Paint::Custom(&CustomPaint {
                    source_id: custom_paint_source.custom_paint_source_id,
                    width,
                    height,
                    scale: self.scale,
                } as &(dyn Any + Send + Sync)),
                None,
                &Rect::from_origin_size((0.0, 0.0), (width as f64, height as f64)),
            );
        }
    }

    fn draw_sub_document(&self, scene: &mut impl PaintScene) {
        if let Some(sub_doc) = self.element.sub_doc_data().map(|doc| doc.inner()) {
            let scale = self.scale;
            let width = self.frame.content_box.width() as u32;
            let height = self.frame.content_box.height() as u32;
            let initial_x = self.pos.x + self.frame.content_box.origin().x;
            let initial_y = self.pos.y + self.frame.content_box.origin().y;
            // let transform = self.transform.then_translate(Vec2 { x, y });

            let painter =
                BlitzDomPainter::new(&sub_doc, scale, width, height, initial_x, initial_y);
            painter.paint_scene(scene);
        }
    }

    fn stroke_devtools(&self, scene: &mut impl PaintScene) {
        if self.devtools.show_layout {
            let shape = &self.frame.border_box;
            let stroke = Stroke::new(self.scale);

            let stroke_color = match self.node.style.display {
                taffy::Display::Block => Color::new([1.0, 0.0, 0.0, 1.0]),
                taffy::Display::Flex => Color::new([0.0, 1.0, 0.0, 1.0]),
                taffy::Display::Grid => Color::new([0.0, 0.0, 1.0, 1.0]),
                taffy::Display::None => Color::new([0.0, 0.0, 1.0, 1.0]),
            };

            scene.stroke(&stroke, self.transform, stroke_color, None, &shape);
        }
    }

    /// Draw all borders for a node
    fn draw_border(&self, scene: &mut impl PaintScene) {
        for edge in [Edge::Top, Edge::Right, Edge::Bottom, Edge::Left] {
            self.draw_border_edge(scene, edge);
        }
    }

    fn draw_table_borders(&self, scene: &mut impl PaintScene) {
        let SpecialElementData::TableRoot(table) = &self.element.special_data else {
            return;
        };
        // Borders are only handled at the table level when BorderCollapse::Collapse
        if table.border_collapse != BorderCollapse::Collapse {
            return;
        }

        let Some(grid_info) = &mut *table.computed_grid_info.borrow_mut() else {
            return;
        };
        let Some(border_style) = table.border_style.as_deref() else {
            return;
        };

        let outer_border_style = self.style.get_border();

        let cols = &grid_info.columns;
        let rows = &grid_info.rows;

        let inner_width =
            (cols.sizes.iter().sum::<f32>() + cols.gutters.iter().sum::<f32>()) as f64;
        let inner_height =
            (rows.sizes.iter().sum::<f32>() + rows.gutters.iter().sum::<f32>()) as f64;

        // TODO: support different colors for different borders
        let current_color = self.style.clone_color();
        let border_color = border_style
            .border_top_color
            .resolve_to_absolute(&current_color)
            .as_srgb_color();

        // No need to draw transparent borders (as they won't be visible anyway)
        if border_color == Color::TRANSPARENT {
            return;
        }

        let border_width = border_style.border_top_width.0.to_f64_px();

        // Draw horizontal inner borders
        let mut y = 0.0;
        for (&height, &gutter) in rows.sizes.iter().zip(rows.gutters.iter()) {
            let shape =
                Rect::new(0.0, y, inner_width, y + gutter as f64).scale_from_origin(self.scale);
            scene.fill(Fill::NonZero, self.transform, border_color, None, &shape);

            y += (height + gutter) as f64;
        }

        // Draw horizontal outer borders
        // Top border
        if outer_border_style.border_top_style != BorderStyle::Hidden {
            let shape =
                Rect::new(0.0, 0.0, inner_width, border_width).scale_from_origin(self.scale);
            scene.fill(Fill::NonZero, self.transform, border_color, None, &shape);
        }
        // Bottom border
        if outer_border_style.border_bottom_style != BorderStyle::Hidden {
            let shape = Rect::new(0.0, inner_height, inner_width, inner_height + border_width)
                .scale_from_origin(self.scale);
            scene.fill(Fill::NonZero, self.transform, border_color, None, &shape);
        }

        // Draw vertical inner borders
        let mut x = 0.0;
        for (&width, &gutter) in cols.sizes.iter().zip(cols.gutters.iter()) {
            let shape =
                Rect::new(x, 0.0, x + gutter as f64, inner_height).scale_from_origin(self.scale);
            scene.fill(Fill::NonZero, self.transform, border_color, None, &shape);

            x += (width + gutter) as f64;
        }

        // Draw vertical outer borders
        // Left border
        if outer_border_style.border_left_style != BorderStyle::Hidden {
            let shape =
                Rect::new(0.0, 0.0, border_width, inner_height).scale_from_origin(self.scale);
            scene.fill(Fill::NonZero, self.transform, border_color, None, &shape);
        }
        // Right border
        if outer_border_style.border_right_style != BorderStyle::Hidden {
            let shape = Rect::new(inner_width, 0.0, inner_width + border_width, inner_height)
                .scale_from_origin(self.scale);
            scene.fill(Fill::NonZero, self.transform, border_color, None, &shape);
        }
    }

    /// Draw a single border edge for a node
    fn draw_border_edge(&self, scene: &mut impl PaintScene, edge: Edge) {
        let style = &*self.style;
        let border = style.get_border();
        let path = self.frame.border_edge_shape(edge);

        let current_color = style.clone_color();
        let color = match edge {
            Edge::Top => border
                .border_top_color
                .resolve_to_absolute(&current_color)
                .as_srgb_color(),
            Edge::Right => border
                .border_right_color
                .resolve_to_absolute(&current_color)
                .as_srgb_color(),
            Edge::Bottom => border
                .border_bottom_color
                .resolve_to_absolute(&current_color)
                .as_srgb_color(),
            Edge::Left => border
                .border_left_color
                .resolve_to_absolute(&current_color)
                .as_srgb_color(),
        };

        let alpha = color.components[3];
        if alpha != 0.0 {
            scene.fill(Fill::NonZero, self.transform, color, None, &path);
        }
    }

    /// ❌ dotted - Defines a dotted border
    /// ❌ dashed - Defines a dashed border
    /// ✅ solid - Defines a solid border
    /// ❌ double - Defines a double border
    /// ❌ groove - Defines a 3D grooved border. The effect depends on the border-color value
    /// ❌ ridge - Defines a 3D ridged border. The effect depends on the border-color value
    /// ❌ inset - Defines a 3D inset border. The effect depends on the border-color value
    /// ❌ outset - Defines a 3D outset border. The effect depends on the border-color value
    /// ✅ none - Defines no border
    /// ✅ hidden - Defines a hidden border
    fn draw_outline(&self, scene: &mut impl PaintScene) {
        let outline = self.style.get_outline();

        let current_color = self.style.clone_color();
        let color = outline
            .outline_color
            .resolve_to_absolute(&current_color)
            .as_srgb_color();

        let style = match outline.outline_style {
            OutlineStyle::Auto => return,
            OutlineStyle::BorderStyle(style) => style,
        };

        let path = match style {
            BorderStyle::None | BorderStyle::Hidden => return,
            BorderStyle::Solid => self.frame.outline(),

            // TODO: Implement other border styles
            BorderStyle::Inset
            | BorderStyle::Groove
            | BorderStyle::Outset
            | BorderStyle::Ridge
            | BorderStyle::Dotted
            | BorderStyle::Dashed
            | BorderStyle::Double => self.frame.outline(),
        };

        scene.fill(Fill::NonZero, self.transform, color, None, &path);
    }
}
impl<'a> std::ops::Deref for ElementCx<'a> {
    type Target = BlitzDomPainter<'a>;
    fn deref(&self) -> &Self::Target {
        self.context
    }
}

fn insets_from_taffy_rect(input: taffy::Rect<f64>) -> Insets {
    Insets {
        x0: input.left,
        y0: input.top,
        x1: input.right,
        y1: input.bottom,
    }
}

/// Convert Stylo and Taffy types into Kurbo types
fn create_css_rect(style: &ComputedValues, layout: &Layout, scale: f64) -> CssBox {
    // Resolve and rescale
    // We have to scale since document pixels are not same same as rendered pixels
    let width: f64 = layout.size.width as f64;
    let height: f64 = layout.size.height as f64;
    let border_box = Rect::new(0.0, 0.0, width * scale, height * scale);
    let border = insets_from_taffy_rect(layout.border.map(|p| p as f64 * scale));
    let padding = insets_from_taffy_rect(layout.padding.map(|p| p as f64 * scale));
    let outline_width = style.get_outline().outline_width.0.to_f64_px() * scale;

    // Resolve the radii to a length. need to downscale since the radii are in document pixels
    let resolve_w = CSSPixelLength::new(width as _);
    let resolve_h = CSSPixelLength::new(height as _);
    let resolve_radii = |radius: &BorderCornerRadius| -> Vec2 {
        Vec2 {
            x: scale * radius.0.width.0.resolve(resolve_w).px() as f64,
            y: scale * radius.0.height.0.resolve(resolve_h).px() as f64,
        }
    };
    let s_border = style.get_border();
    let border_radii = NonUniformRoundedRectRadii {
        top_left: resolve_radii(&s_border.border_top_left_radius),
        top_right: resolve_radii(&s_border.border_top_right_radius),
        bottom_right: resolve_radii(&s_border.border_bottom_right_radius),
        bottom_left: resolve_radii(&s_border.border_bottom_left_radius),
    };

    CssBox::new(border_box, border, padding, outline_width, border_radii)
}
