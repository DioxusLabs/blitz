use std::sync::Arc;

use super::multicolor_rounded_rect::{Edge, ElementFrame};
use crate::{
    devtools::Devtools,
    util::{GradientSlice, StyloGradient, ToVelloColor},
};
use blitz_dom::node::{NodeData, TextBrush, TextInputData, TextNodeData};
use blitz_dom::{local_name, Document, Node};

use style::{
    dom::TElement,
    properties::{
        generated::longhands::visibility::computed_value::T as StyloVisibility,
        style_structs::{Font, Outline},
        ComputedValues,
    },
    values::{
        computed::{
            Angle, AngleOrPercentage, CSSPixelLength, LengthPercentage, LineDirection, Percentage,
        },
        generics::{
            color::Color as StyloColor,
            image::{
                EndingShape, GenericGradient, GenericGradientItem, GenericImage, GradientFlags,
            },
            position::GenericPosition,
            NonNegative,
        },
        specified::{
            position::{HorizontalPositionKeyword, VerticalPositionKeyword},
            BorderStyle, OutlineStyle,
        },
    },
    OwnedSlice,
};

use image::{imageops::FilterType, DynamicImage};
use parley::layout::PositionedLayoutItem;
use taffy::prelude::Layout;
use vello::{
    kurbo::{Affine, Point, Rect, Shape, Stroke, Vec2},
    peniko::{self, Brush, Color, Fill},
    Scene,
};
use vello_svg::usvg;

/// Draw the current tree to current render surface
/// Eventually we'll want the surface itself to be passed into the render function, along with things like the viewport
///
/// This assumes styles are resolved and layout is complete.
/// Make sure you do those before trying to render
pub fn generate_vello_scene(
    scene: &mut Scene,
    dom: &Document,
    scale: f64,
    devtool_config: Devtools,
) {
    let generator = VelloSceneGenerator {
        dom,
        scale,
        devtools: devtool_config,
        scroll_offset: dom.scroll_offset,
    };
    generator.generate_vello_scene(scene)
}

/// A short-lived struct which holds a bunch of parameters for rendering a vello scene so
/// that we don't have to pass them down as parameters
pub struct VelloSceneGenerator<'dom> {
    /// Input parameters (read only) for generating the Scene
    dom: &'dom Document,
    scale: f64,
    devtools: Devtools,
    scroll_offset: f64,
}

impl<'dom> VelloSceneGenerator<'dom> {
    fn node_position(&self, node: usize, location: Point) -> (Layout, Point) {
        let layout = self.layout(node);
        let pos = location + Vec2::new(layout.location.x as f64, layout.location.y as f64);
        (layout, pos)
    }

    fn layout(&self, child: usize) -> Layout {
        self.dom.as_ref().tree()[child].unrounded_layout
        // self.dom.tree()[child].final_layout
    }

    /// Draw the current tree to current render surface
    /// Eventually we'll want the surface itself to be passed into the render function, along with things like the viewport
    ///
    /// This assumes styles are resolved and layout is complete.
    /// Make sure you do those before trying to render
    pub fn generate_vello_scene(&self, scene: &mut Scene) {
        // Simply render the document (the root element (note that this is not the same as the root node)))
        scene.reset();
        self.render_element(
            scene,
            self.dom.as_ref().root_element().id,
            Point {
                x: 0.0,
                y: self.scroll_offset,
            },
        );

        // Render debug overlay
        if self.devtools.highlight_hover {
            if let Some(node_id) = self.dom.as_ref().get_hover_node_id() {
                self.render_debug_overlay(scene, node_id);
            }
        }
    }

    /// Renders a layout debugging overlay which visualises the content size, padding and border
    /// of the node with a transparent overlay.
    fn render_debug_overlay(&self, scene: &mut Scene, node_id: usize) {
        let scale = self.scale;

        let mut node = &self.dom.as_ref().tree()[node_id];

        let taffy::Layout {
            size,
            border,
            padding,
            ..
        } = node.final_layout;
        let taffy::Size { width, height } = size;

        let padding_border = padding + border;
        let scaled_pb = padding_border.map(|v| f64::from(v) * scale);
        let scaled_padding = padding.map(|v| f64::from(v) * scale);
        let scaled_border = border.map(|v| f64::from(v) * scale);

        let content_width = width - padding_border.left - padding_border.right;
        let content_height = height - padding_border.top - padding_border.bottom;

        let taffy::Point { x, y } = node.final_layout.location;

        let mut abs_x = x;
        let mut abs_y = y;
        while let Some(parent_id) = node.parent {
            node = &self.dom.as_ref().tree()[parent_id];
            let taffy::Point { x, y } = node.final_layout.location;
            abs_x += x;
            abs_y += y;
        }

        abs_y += self.scroll_offset as f32;

        // Hack: scale factor
        let abs_x = f64::from(abs_x) * scale;
        let abs_y = f64::from(abs_y) * scale;
        let width = f64::from(width) * scale;
        let height = f64::from(height) * scale;
        let content_width = f64::from(content_width) * scale;
        let content_height = f64::from(content_height) * scale;

        // Fill content box blue
        let base_translation = Vec2::new(abs_x, abs_y);
        let transform =
            Affine::translate(base_translation + Vec2::new(scaled_pb.left, scaled_pb.top));
        let rect = Rect::new(0.0, 0.0, content_width, content_height);
        let fill_color = Color::rgba(66.0 / 255.0, 144.0 / 255.0, 245.0 / 255.0, 0.5); // blue
        scene.fill(
            vello::peniko::Fill::NonZero,
            transform,
            fill_color,
            None,
            &rect,
        );

        fn draw_cutout_rect(
            scene: &mut Scene,
            base_translation: Vec2,
            size: Vec2,
            edge_widths: taffy::Rect<f64>,
            color: Color,
        ) {
            let mut fill = |pos: Vec2, width: f64, height: f64| {
                scene.fill(
                    vello::peniko::Fill::NonZero,
                    Affine::translate(pos),
                    color,
                    None,
                    &Rect::new(0.0, 0.0, width, height),
                );
            };

            let right = size.x - edge_widths.right;
            let bottom = size.y - edge_widths.bottom;
            let inner_h = size.y - edge_widths.top - edge_widths.bottom;
            let inner_w = size.x - edge_widths.left - edge_widths.right;

            let bt = base_translation;
            let ew = edge_widths;

            // Corners
            fill(bt, ew.left, ew.top); // top-left
            fill(bt + Vec2::new(0.0, bottom), ew.left, ew.bottom); // bottom-left
            fill(bt + Vec2::new(right, 0.0), ew.right, ew.top); // top-right
            fill(bt + Vec2::new(right, bottom), ew.right, ew.bottom); // bottom-right

            // Sides
            fill(bt + Vec2::new(0.0, ew.top), ew.left, inner_h); // left
            fill(bt + Vec2::new(right, ew.top), ew.right, inner_h); // right
            fill(bt + Vec2::new(ew.left, 0.0), inner_w, ew.top); // top
            fill(bt + Vec2::new(ew.left, bottom), inner_w, ew.bottom); // bottom
        }

        let padding_color = Color::rgba(81.0 / 255.0, 144.0 / 245.0, 66.0 / 255.0, 0.5); // green
        draw_cutout_rect(
            scene,
            base_translation + Vec2::new(scaled_border.left, scaled_border.top),
            Vec2::new(
                content_width + scaled_padding.left + scaled_padding.right,
                content_height + scaled_padding.top + scaled_padding.bottom,
            ),
            scaled_padding.map(f64::from),
            padding_color,
        );

        let border_color = Color::rgba(245.0 / 255.0, 66.0 / 245.0, 66.0 / 255.0, 0.5); // red
        draw_cutout_rect(
            scene,
            base_translation,
            Vec2::new(width, height),
            scaled_border.map(f64::from),
            border_color,
        );
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
    fn render_element(&self, scene: &mut Scene, node_id: usize, location: Point) {
        // Need to do research on how we can cache most of the bezpaths - there's gonna be a lot of encoding between frames.
        // Might be able to cache resources deeper in vello.
        //
        // Implemented (completely):
        //  - nothing is completely done:
        //  - vello is limiting all the styles we can implement (performantly)
        //  - servo is missing a number of features (like space-evenly justify)
        //
        // Implemented (partially):
        //  - background, border, font, margin, outline, padding,
        //
        // Not Implemented:
        //  - list, position, table, text, ui,
        //  - custom_properties, writing_mode, rules, visited_style, flags,  box_, column, counters, effects,
        //  - inherited_box, inherited_table, inherited_text, inherited_ui,
        let element = &self.dom.as_ref().tree()[node_id];

        // Early return if the element is hidden
        if matches!(element.style.display, taffy::prelude::Display::None) {
            return;
        }

        // Only draw elements with a style
        if element.primary_styles().is_none() {
            return;
        }

        // Hide elements with "hidden" attribute
        if let Some("true" | "") = element.attr(local_name!("hidden")) {
            return;
        }

        // Hide inputs with type=hidden
        // Implemented here rather than using the style engine for performance reasons
        if element.local_name() == "input" && element.attr(local_name!("type")) == Some("hidden") {
            return;
        }

        // Hide elements with a visibility style other than visible
        if element
            .primary_styles()
            .unwrap()
            .get_inherited_box()
            .visibility
            != StyloVisibility::Visible
        {
            return;
        }

        // We can't fully support opacity yet, but we can hide elements with opacity 0
        if element.primary_styles().unwrap().get_effects().opacity == 0.0 {
            return;
        }

        let cx = self.element_cx(element, location);
        cx.stroke_effects(scene);
        cx.stroke_outline(scene);
        cx.stroke_frame(scene);
        cx.stroke_border(scene);
        cx.stroke_devtools(scene);
        cx.draw_image(scene);
        cx.draw_svg(scene);

        // Render the text in text inputs
        if let Some(input_data) = cx.text_input {
            let (_layout, pos) = self.node_position(node_id, location);
            let text_layout = input_data.editor.layout();

            // Apply padding/border offset to inline root
            let taffy::Layout {
                border, padding, ..
            } = element.final_layout;
            let scaled_pb = (padding + border).map(f64::from);
            let pos = vello::kurbo::Point {
                x: pos.x + scaled_pb.left,
                y: pos.y + scaled_pb.top,
            };

            // Render text
            cx.stroke_text(scene, text_layout, pos);

            // Render caret
            let cursor_line = input_data.editor.get_cursor_line();
            let transform = Affine::translate((pos.x * self.scale, pos.y * self.scale));
            if let Some(line) = cursor_line {
                scene.stroke(
                    &Stroke::new(2.),
                    transform,
                    &Brush::Solid(Color::BLACK),
                    None,
                    &line,
                );
            }

            return;
        }

        if element.is_inline_root {
            let (_layout, pos) = self.node_position(node_id, location);
            let text_layout = &element
                .raw_dom_data
                .downcast_element()
                .unwrap()
                .inline_layout_data()
                .unwrap_or_else(|| {
                    panic!("Tried to render node marked as inline root that does not have an inline layout: {:?}", element);
                });

            // Apply padding/border offset to inline root
            let taffy::Layout {
                border, padding, ..
            } = element.final_layout;
            let scaled_pb = (padding + border).map(f64::from);
            let pos = vello::kurbo::Point {
                x: pos.x + scaled_pb.left,
                y: pos.y + scaled_pb.top,
            };

            // Render text
            cx.stroke_text(scene, &text_layout.layout, pos);

            // Render inline boxes
            for line in text_layout.layout.lines() {
                for item in line.items() {
                    if let PositionedLayoutItem::InlineBox(ibox) = item {
                        self.render_node(scene, ibox.id as usize, pos);
                    }
                }
            }
        } else {
            for child_id in cx
                .element
                .layout_children
                .borrow()
                .as_ref()
                .unwrap()
                .iter()
                .copied()
            {
                self.render_node(scene, child_id, cx.pos);
            }
        }
    }

    fn render_node(&self, scene: &mut Scene, node_id: usize, location: Point) {
        let node = &self.dom.as_ref().tree()[node_id];

        match &node.raw_dom_data {
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

    fn element_cx<'w>(&'w self, element: &'w Node, location: Point) -> ElementCx {
        let style = element
            .stylo_element_data
            .borrow()
            .as_ref()
            .map(|element_data| element_data.styles.primary().clone())
            .unwrap_or(
                ComputedValues::initial_values_with_font_override(Font::initial_values()).to_arc(),
            );

        let (layout, pos) = self.node_position(element.id, location);
        let scale = self.scale;

        // the bezpaths for every element are (potentially) cached (not yet, tbd)
        // By performing the transform, we prevent the cache from becoming invalid when the page shifts around
        let transform = Affine::translate((pos.x * scale, pos.y * scale));

        // todo: maybe cache this so we don't need to constantly be figuring it out
        // It is quite a bit of math to calculate during render/traverse
        // Also! we can cache the bezpaths themselves, saving us a bunch of work
        let frame = ElementFrame::new(&style, &layout, scale);

        ElementCx {
            frame,
            scale,
            style,
            pos,
            element,
            transform,
            image: element
                .element_data()
                .unwrap()
                .image_data()
                .map(|data| &*data.image),
            svg: element.element_data().unwrap().svg_data(),
            text_input: element.element_data().unwrap().text_input_data(),
            devtools: &self.devtools,
        }
    }
}

/// A context of loaded and hot data to draw the element from
struct ElementCx<'a> {
    frame: ElementFrame,
    style: style::servo_arc::Arc<ComputedValues>,
    pos: Point,
    scale: f64,
    element: &'a Node,
    transform: Affine,
    image: Option<&'a DynamicImage>,
    svg: Option<&'a usvg::Tree>,
    text_input: Option<&'a TextInputData>,
    devtools: &'a Devtools,
}

impl ElementCx<'_> {
    fn stroke_text(&self, scene: &mut Scene, text_layout: &parley::Layout<TextBrush>, pos: Point) {
        let transform = Affine::translate((pos.x * self.scale, pos.y * self.scale));

        for line in text_layout.lines() {
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(glyph_run) = item {
                    let mut x = glyph_run.offset();
                    let y = glyph_run.baseline();
                    let run = glyph_run.run();
                    let font = run.font();
                    let font_size = run.font_size();
                    let metrics = run.metrics();
                    let style = glyph_run.style();
                    let synthesis = run.synthesis();
                    let glyph_xform = synthesis
                        .skew()
                        .map(|angle| Affine::skew(angle.to_radians().tan() as f64, 0.0));
                    let coords = run
                        .normalized_coords()
                        .iter()
                        .map(|coord| vello::skrifa::instance::NormalizedCoord::from_bits(*coord))
                        .collect::<Vec<_>>();

                    let text_brush = match &style.brush {
                        TextBrush::Normal(text_brush) => text_brush,
                        TextBrush::Highlight { text, fill } => {
                            scene.fill(
                                Fill::EvenOdd,
                                transform,
                                fill,
                                None,
                                &Rect::from_origin_size(
                                    (
                                        glyph_run.offset() as f64,
                                        // The y coordinate is on the baseline. We want to draw from the top of the line
                                        // (Note that we are in a y-down coordinate system)
                                        (y - metrics.ascent - metrics.leading) as f64,
                                    ),
                                    (
                                        glyph_run.advance() as f64,
                                        (metrics.ascent + metrics.descent + metrics.leading) as f64,
                                    ),
                                ),
                            );

                            text
                        }
                    };

                    scene
                        .draw_glyphs(font)
                        .brush(text_brush)
                        .transform(transform)
                        .glyph_transform(glyph_xform)
                        .font_size(font_size)
                        .normalized_coords(&coords)
                        .draw(
                            Fill::NonZero,
                            glyph_run.glyphs().map(|glyph| {
                                let gx = x + glyph.x;
                                let gy = y - glyph.y;
                                x += glyph.advance;
                                vello::glyph::Glyph {
                                    id: glyph.id as _,
                                    x: gx,
                                    y: gy,
                                }
                            }),
                        );

                    let mut draw_decoration_line = |offset: f32, size: f32, brush: &TextBrush| {
                        let x = glyph_run.offset() as f64;
                        let w = glyph_run.advance() as f64;
                        let y = (glyph_run.baseline() - offset + size / 2.0) as f64;
                        let line = vello::kurbo::Line::new((x, y), (x + w, y));
                        scene.stroke(
                            &Stroke::new(size as f64),
                            transform,
                            brush.text_brush(),
                            None,
                            &line,
                        )
                    };

                    if let Some(underline) = &style.underline {
                        let offset = underline.offset.unwrap_or(metrics.underline_offset);
                        let size = underline.size.unwrap_or(metrics.underline_size);

                        // TODO: intercept line when crossing an descending character like "gqy"
                        draw_decoration_line(offset, size, &underline.brush);
                    }
                    if let Some(strikethrough) = &style.strikethrough {
                        let offset = strikethrough.offset.unwrap_or(metrics.strikethrough_offset);
                        let size = strikethrough.size.unwrap_or(metrics.strikethrough_size);

                        draw_decoration_line(offset, size, &strikethrough.brush);
                    }
                }
            }
        }
    }

    fn draw_svg(&self, scene: &mut Scene) {
        let transform = Affine::translate((self.pos.x * self.scale, self.pos.y * self.scale))
            .pre_scale(self.scale);
        if let Some(svg) = self.svg {
            let fragment = vello_svg::render_tree(svg);
            scene.append(&fragment, Some(transform));
        }
    }

    fn draw_image(&self, scene: &mut Scene) {
        let transform = Affine::translate((self.pos.x * self.scale, self.pos.y * self.scale));

        let width = self.frame.inner_rect.width() as u32;
        let height = self.frame.inner_rect.height() as u32;

        if let Some(image) = self.image {
            let mut resized_image = self
                .element
                .element_data()
                .unwrap()
                .image_data()
                .unwrap()
                .resized_image
                .borrow_mut();

            if resized_image.is_none()
                || resized_image
                    .as_ref()
                    .is_some_and(|img| img.width != width || img.height != height)
            {
                let image_data = image
                    .clone()
                    .resize_to_fill(width, height, FilterType::Lanczos3)
                    .into_rgba8()
                    .into_raw();

                let peniko_image = peniko::Image {
                    data: peniko::Blob::new(Arc::new(image_data)),
                    format: peniko::Format::Rgba8,
                    width,
                    height,
                    extend: peniko::Extend::Pad,
                };

                *resized_image = Some(Arc::new(peniko_image));
            }

            scene.draw_image(resized_image.as_ref().unwrap(), transform);
        }
    }

    fn stroke_devtools(&self, scene: &mut Scene) {
        if self.devtools.show_layout {
            let shape = &self.frame.outer_rect;
            let stroke = Stroke::new(self.scale);

            let stroke_color = match self.element.style.display {
                taffy::prelude::Display::Block => Color::rgb(1.0, 0.0, 0.0),
                taffy::prelude::Display::Flex => Color::rgb(0.0, 1.0, 0.0),
                taffy::prelude::Display::Grid => Color::rgb(0.0, 0.0, 1.0),
                taffy::prelude::Display::None => Color::rgb(0.0, 0.0, 1.0),
            };

            scene.stroke(&stroke, self.transform, stroke_color, None, &shape);
        }

        // if self.devtools.show_style {
        //     self.frame.draw_style(scene);
        // }

        // if self.devtools.print_hover {
        //     self.frame.draw_hover(scene);
        // }
    }

    fn stroke_frame(&self, scene: &mut Scene) {
        use GenericImage::*;

        for segment in &self.style.get_background().background_image.0 {
            match segment {
                None => self.draw_solid_frame(scene),
                Gradient(gradient) => self.draw_gradient_frame(scene, gradient),
                Url(_) => {
                    //
                    // todo!("Implement background drawing for Image::Url")
                    println!("Implement background drawing for Image::Url");
                    // let background = self.style.get_background();

                    // todo: handle non-absolute colors
                    // let bg_color = background.background_color.clone();
                    // let bg_color = bg_color.as_absolute().unwrap();
                    // let bg_color = Color::RED;
                    // let shape = self.frame.outer_rect;

                    // // Fill the color
                    // scene.fill(
                    //     Fill::NonZero,
                    //     self.transform,
                    //     Color::RED,
                    //     // bg_color.as_vello(),
                    //     Option::None,
                    //     &shape,
                    // );
                }
                PaintWorklet(_) => todo!("Implement background drawing for Image::PaintWorklet"),
                CrossFade(_) => todo!("Implement background drawing for Image::CrossFade"),
                ImageSet(_) => todo!("Implement background drawing for Image::ImageSet"),
            }
        }
    }

    fn draw_gradient_frame(&self, scene: &mut Scene, gradient: &StyloGradient) {
        match gradient {
            // https://developer.mozilla.org/en-US/docs/Web/CSS/gradient/linear-gradient
            GenericGradient::Linear {
                direction,
                items,
                // repeating,
                // compat_mode,
                ..
            } => self.draw_linear_gradient(scene, direction, items),
            GenericGradient::Radial {
                shape,
                position,
                items,
                flags,
                // compat_mode,
                ..
            } => self.draw_radial_gradient(scene, shape, position, items, *flags),
            GenericGradient::Conic {
                angle,
                position,
                items,
                flags,
                ..
            } => self.draw_conic_gradient(scene, angle, position, items, *flags),
        };
    }

    fn draw_linear_gradient(
        &self,
        scene: &mut Scene,
        direction: &LineDirection,
        items: &GradientSlice,
    ) {
        let bb = self.frame.outer_rect.bounding_box();

        let shape = self.frame.frame();
        let center = bb.center();
        let rect = self.frame.inner_rect;
        let (start, end) = match direction {
            LineDirection::Angle(angle) => {
                let start = Point::new(
                    self.frame.inner_rect.x0 + rect.width() / 2.0,
                    self.frame.inner_rect.y0,
                );
                let end = Point::new(
                    self.frame.inner_rect.x0 + rect.width() / 2.0,
                    self.frame.inner_rect.y1,
                );

                // rotate the lind around the center
                let line = Affine::rotate_about(-angle.radians64(), center)
                    * vello::kurbo::Line::new(start, end);

                (line.p0, line.p1)
            }
            LineDirection::Horizontal(horizontal) => {
                let start = Point::new(
                    self.frame.inner_rect.x0,
                    self.frame.inner_rect.y0 + rect.height() / 2.0,
                );
                let end = Point::new(
                    self.frame.inner_rect.x1,
                    self.frame.inner_rect.y0 + rect.height() / 2.0,
                );
                match horizontal {
                    HorizontalPositionKeyword::Right => (start, end),
                    HorizontalPositionKeyword::Left => (end, start),
                }
            }
            LineDirection::Vertical(vertical) => {
                let start = Point::new(
                    self.frame.inner_rect.x0 + rect.width() / 2.0,
                    self.frame.inner_rect.y0,
                );
                let end = Point::new(
                    self.frame.inner_rect.x0 + rect.width() / 2.0,
                    self.frame.inner_rect.y1,
                );
                match vertical {
                    VerticalPositionKeyword::Top => (end, start),
                    VerticalPositionKeyword::Bottom => (start, end),
                }
            }
            LineDirection::Corner(horizontal, vertical) => {
                let (start_x, end_x) = match horizontal {
                    HorizontalPositionKeyword::Right => {
                        (self.frame.inner_rect.x0, self.frame.inner_rect.x1)
                    }
                    HorizontalPositionKeyword::Left => {
                        (self.frame.inner_rect.x1, self.frame.inner_rect.x0)
                    }
                };
                let (start_y, end_y) = match vertical {
                    VerticalPositionKeyword::Top => {
                        (self.frame.inner_rect.y1, self.frame.inner_rect.y0)
                    }
                    VerticalPositionKeyword::Bottom => {
                        (self.frame.inner_rect.y0, self.frame.inner_rect.y1)
                    }
                };
                (Point::new(start_x, start_y), Point::new(end_x, end_y))
            }
        };
        let mut gradient = peniko::Gradient {
            kind: peniko::GradientKind::Linear { start, end },
            extend: Default::default(),
            stops: Default::default(),
        };

        let mut hint: Option<f32> = None;

        for (idx, item) in items.iter().enumerate() {
            let (color, offset) = match item {
                GenericGradientItem::SimpleColorStop(color) => {
                    let step = 1.0 / (items.len() as f32 - 1.0);
                    let offset = step * idx as f32;
                    let color = color.as_vello();
                    (color, offset)
                }
                GenericGradientItem::ComplexColorStop { color, position } => {
                    match position.to_percentage().map(|pos| pos.0) {
                        Some(offset) => {
                            let color = color.as_vello();
                            (color, offset)
                        }
                        // TODO: implement absolute and calc stops
                        None => continue,
                    }
                }
                GenericGradientItem::InterpolationHint(position) => {
                    hint = match position.to_percentage() {
                        Some(Percentage(percentage)) => Some(percentage),
                        _ => None,
                    };
                    continue;
                }
            };

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
                        let mid_offset = last_stop.offset * (1.0 - hint) + offset * hint;
                        let multiplier = hint.powf(0.5f32.log(mid_offset));
                        let mid_color = Color::rgba8(
                            (last_stop.color.r as f32
                                + multiplier * (color.r as f32 - last_stop.color.r as f32))
                                as u8,
                            (last_stop.color.g as f32
                                + multiplier * (color.g as f32 - last_stop.color.g as f32))
                                as u8,
                            (last_stop.color.b as f32
                                + multiplier * (color.b as f32 - last_stop.color.b as f32))
                                as u8,
                            (last_stop.color.a as f32
                                + multiplier * (color.a as f32 - last_stop.color.a as f32))
                                as u8,
                        );
                        tracing::info!("Gradient stop {:?}", mid_color);
                        gradient.stops.push(peniko::ColorStop {
                            color: mid_color,
                            offset: mid_offset,
                        });
                        gradient.stops.push(peniko::ColorStop { color, offset });
                    }
                }
            }
        }
        let brush = peniko::BrushRef::Gradient(&gradient);
        scene.fill(peniko::Fill::NonZero, self.transform, brush, None, &shape);
    }

    // fn draw_image_frame(&self, scene: &mut Scene) {}

    fn draw_solid_frame(&self, scene: &mut Scene) {
        let background = self.style.get_background();

        let bg_color = background.background_color.as_vello();
        let shape = self.frame.frame();

        // Fill the color
        scene.fill(Fill::NonZero, self.transform, bg_color, None, &shape);
    }

    /// Stroke a border
    ///
    /// The border-style property specifies what kind of border to display.
    ///
    /// The following values are allowed:
    /// ❌ dotted - Defines a dotted border
    /// ❌ dashed - Defines a dashed border
    /// ✅ solid - Defines a solid border
    /// ❌ double - Defines a double border
    /// ❌ groove - Defines a 3D grooved border.
    /// ❌ ridge - Defines a 3D ridged border.
    /// ❌ inset - Defines a 3D inset border.
    /// ❌ outset - Defines a 3D outset border.
    /// ✅ none - Defines no border
    /// ✅ hidden - Defines a hidden border
    ///
    /// The border-style property can have from one to four values (for the top border, right border, bottom border, and the left border).
    fn stroke_border(&self, sb: &mut Scene) {
        for edge in [Edge::Top, Edge::Right, Edge::Bottom, Edge::Left] {
            self.stroke_border_edge(sb, edge);
        }
    }

    /// The border-style property specifies what kind of border to display.
    ///
    /// [Border](https://www.w3schools.com/css/css_border.asp)
    ///
    /// The following values are allowed:
    /// - ❌ dotted: Defines a dotted border
    /// - ❌ dashed: Defines a dashed border
    /// - ✅ solid: Defines a solid border
    /// - ❌ double: Defines a double border
    /// - ❌ groove: Defines a 3D grooved border*
    /// - ❌ ridge: Defines a 3D ridged border*
    /// - ❌ inset: Defines a 3D inset border*
    /// - ❌ outset: Defines a 3D outset border*
    /// - ✅ none: Defines no border
    /// - ✅ hidden: Defines a hidden border
    ///
    /// [*] The effect depends on the border-color value
    fn stroke_border_edge(&self, sb: &mut Scene, edge: Edge) {
        let border = self.style.get_border();
        let path = self.frame.border(edge);

        let color = match edge {
            Edge::Top => border.border_top_color.as_vello(),
            Edge::Right => border.border_right_color.as_vello(),
            Edge::Bottom => border.border_bottom_color.as_vello(),
            Edge::Left => border.border_left_color.as_vello(),
        };

        sb.fill(Fill::NonZero, self.transform, color, None, &path);
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
    fn stroke_outline(&self, scene: &mut Scene) {
        let Outline {
            outline_color,
            outline_style,
            ..
        } = self.style.get_outline();

        let color = outline_color
            .as_absolute()
            .map(ToVelloColor::as_vello)
            .unwrap_or_default();

        let style = match outline_style {
            OutlineStyle::Auto => return,
            OutlineStyle::BorderStyle(BorderStyle::Hidden) => return,
            OutlineStyle::BorderStyle(BorderStyle::None) => return,
            OutlineStyle::BorderStyle(style) => style,
        };

        let path = match style {
            BorderStyle::None | BorderStyle::Hidden => return,
            BorderStyle::Solid => self.frame.outline(),

            // TODO: Implement other border styles
            BorderStyle::Inset => self.frame.outline(),
            BorderStyle::Groove => self.frame.outline(),
            BorderStyle::Outset => self.frame.outline(),
            BorderStyle::Ridge => self.frame.outline(),
            BorderStyle::Dotted => self.frame.outline(),
            BorderStyle::Dashed => self.frame.outline(),
            BorderStyle::Double => self.frame.outline(),
        };

        scene.fill(Fill::NonZero, self.transform, color, None, &path);
    }

    /// Applies filters to a final frame
    ///
    /// Notably, I don't think we can do this here since vello needs to run this as a pass (shadows need to apply everywhere)
    ///
    /// ❌ opacity: The opacity computed value.
    /// ❌ box_shadow: The box-shadow computed value.
    /// ❌ clip: The clip computed value.
    /// ❌ filter: The filter computed value.
    /// ❌ mix_blend_mode: The mix-blend-mode computed value.
    fn stroke_effects(&self, _scene: &mut Scene) {
        // also: if focused, draw a focus ring
        //
        //             let stroke_color = Color::rgb(1.0, 1.0, 1.0);
        //             let stroke = Stroke::new(FOCUS_BORDER_WIDTH as f32 / 2.0);
        //             scene_builder.stroke(&stroke, Affine::IDENTITY, stroke_color, None, &shape);
        //             let smaller_rect = shape.rect().inset(-FOCUS_BORDER_WIDTH / 2.0);
        //             let smaller_shape = RoundedRect::from_rect(smaller_rect, shape.radii());
        //             let stroke_color = Color::rgb(0.0, 0.0, 0.0);
        //             scene_builder.stroke(&stroke, Affine::IDENTITY, stroke_color, None, &shape);
        //             background.draw_shape(scene_builder, &smaller_shape, layout, viewport_size);
        // let effects = self.style.get_effects();
    }

    // fn stroke_box_shadow(&self, scene: &mut Scene) {
    //     let effects = self.style.get_effects();
    // }

    fn draw_radial_gradient(
        &self,
        _scene: &mut Scene,
        _shape: &EndingShape<NonNegative<CSSPixelLength>, NonNegative<LengthPercentage>>,
        _position: &GenericPosition<LengthPercentage, LengthPercentage>,
        _items: &OwnedSlice<GenericGradientItem<StyloColor<Percentage>, LengthPercentage>>,
        _flags: GradientFlags,
    ) {
        unimplemented!()
    }

    fn draw_conic_gradient(
        &self,
        _scene: &mut Scene,
        _angle: &Angle,
        _position: &GenericPosition<LengthPercentage, LengthPercentage>,
        _items: &OwnedSlice<GenericGradientItem<StyloColor<Percentage>, AngleOrPercentage>>,
        _flags: GradientFlags,
    ) {
        unimplemented!()
    }
}
