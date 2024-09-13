use std::sync::atomic::{self, AtomicUsize};
use std::sync::Arc;

use super::multicolor_rounded_rect::{Edge, ElementFrame};
use crate::{
    devtools::Devtools,
    util::{GradientSlice, StyloGradient, ToVelloColor},
};
use blitz_dom::node::{
    ListItemLayout, ListItemLayoutPosition, NodeData, TextBrush, TextInputData, TextNodeData,
};
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
            Angle, AngleOrPercentage, CSSPixelLength, LengthPercentage, LineDirection, Overflow,
            Percentage,
        },
        generics::{
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
use style::values::generics::color::GenericColor;
use style::values::generics::image::{
    GenericCircle, GenericEllipse, GenericEndingShape, ShapeExtent,
};
use style::values::specified::percentage::ToPercentage;
use taffy::prelude::Layout;
use vello::kurbo::{BezPath, Cap, Join};
use vello::peniko::Gradient;
use vello::{
    kurbo::{Affine, Point, Rect, Shape, Stroke, Vec2},
    peniko::{self, Brush, Color, Fill, Mix},
    Scene,
};
use vello_svg::usvg;

const CLIP_LIMIT: usize = 1024;
static CLIPS_USED: AtomicUsize = AtomicUsize::new(0);
static CLIP_DEPTH: AtomicUsize = AtomicUsize::new(0);
static CLIP_DEPTH_USED: AtomicUsize = AtomicUsize::new(0);
static CLIPS_WANTED: AtomicUsize = AtomicUsize::new(0);

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
    CLIPS_USED.store(0, atomic::Ordering::SeqCst);
    CLIPS_WANTED.store(0, atomic::Ordering::SeqCst);

    let generator = VelloSceneGenerator {
        dom,
        scale,
        devtools: devtool_config,
    };
    generator.generate_vello_scene(scene);

    // println!(
    //     "Rendered using {} clips (depth: {}) (wanted: {})",
    //     CLIPS_USED.load(atomic::Ordering::SeqCst),
    //     CLIP_DEPTH_USED.load(atomic::Ordering::SeqCst),
    //     CLIPS_WANTED.load(atomic::Ordering::SeqCst)
    // );
}

/// A short-lived struct which holds a bunch of parameters for rendering a vello scene so
/// that we don't have to pass them down as parameters
pub struct VelloSceneGenerator<'dom> {
    /// Input parameters (read only) for generating the Scene
    dom: &'dom Document,
    scale: f64,
    devtools: Devtools,
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
        let viewport_scroll = self.dom.as_ref().viewport_scroll();
        self.render_element(
            scene,
            self.dom.as_ref().root_element().id,
            Point {
                x: -viewport_scroll.x,
                y: -viewport_scroll.y,
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
        //  - position, table, text, ui,
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

        // TODO: account for overflow_x vs overflow_y
        let styles = &element.primary_styles().unwrap();
        let overflow_x = styles.get_box().overflow_x;
        let overflow_y = styles.get_box().overflow_y;
        let should_clip =
            !matches!(overflow_x, Overflow::Visible) || !matches!(overflow_y, Overflow::Visible);
        let clips_available = CLIPS_USED.load(atomic::Ordering::SeqCst) <= CLIP_LIMIT;

        // Apply padding/border offset to inline root
        let (_layout, pos) = self.node_position(node_id, location);
        let taffy::Layout {
            size,
            border,
            padding,
            ..
        } = element.final_layout;
        let scaled_pb = (padding + border).map(f64::from);
        let pos = vello::kurbo::Point {
            x: pos.x + scaled_pb.left,
            y: pos.y + scaled_pb.top,
        };
        let size = vello::kurbo::Size {
            width: (size.width as f64 - scaled_pb.left - scaled_pb.right) * self.scale,
            height: (size.height as f64 - scaled_pb.top - scaled_pb.bottom) * self.scale,
        };
        let transform = Affine::translate((pos.x * self.scale, pos.y * self.scale));
        let origin = vello::kurbo::Point { x: 0.0, y: 0.0 };
        let clip = Rect::from_origin_size(origin, size);

        // Optimise zero-area (/very small area) clips by not rendering at all
        if should_clip && clip.area() < 0.01 {
            return;
        }

        if should_clip {
            CLIPS_WANTED.fetch_add(1, atomic::Ordering::SeqCst);
        }
        if should_clip && clips_available {
            scene.push_layer(Mix::Clip, 1.0, transform, &clip);
            CLIPS_USED.fetch_add(1, atomic::Ordering::SeqCst);
            let depth = CLIP_DEPTH.fetch_add(1, atomic::Ordering::SeqCst) + 1;
            CLIP_DEPTH_USED.fetch_max(depth, atomic::Ordering::SeqCst);
        }

        let mut cx = self.element_cx(element, location);
        cx.stroke_effects(scene);
        cx.stroke_outline(scene);
        cx.draw_outset_box_shadow(scene);
        cx.stroke_frame(scene);
        cx.draw_inset_box_shadow(scene);
        cx.stroke_border(scene);
        cx.stroke_devtools(scene);

        // Now that background has been drawn, offset pos and cx in order to draw our contents scrolled
        let pos = Point {
            x: pos.x - element.scroll_offset.x,
            y: pos.y - element.scroll_offset.y,
        };
        cx.pos = Point {
            x: cx.pos.x - element.scroll_offset.x,
            y: cx.pos.y - element.scroll_offset.y,
        };
        cx.transform = cx.transform.then_translate(Vec2 {
            x: -element.scroll_offset.x,
            y: -element.scroll_offset.y,
        });
        cx.draw_image(scene);
        cx.draw_svg(scene);
        cx.draw_input(scene);

        // Render the text in text inputs
        if let Some(input_data) = cx.text_input {
            let text_layout = input_data.editor.layout();

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
        } else if let Some(ListItemLayout {
            marker: _,
            position: ListItemLayoutPosition::Outside(layout),
        }) = cx.list_item
        {
            //Right align and pad the bullet when rendering outside
            let pos = Point {
                x: pos.x - (layout.full_width() / layout.scale()) as f64,
                y: pos.y,
            };
            cx.stroke_text(scene, layout, pos);
        }

        if element.is_inline_root {
            let text_layout = &element
                .raw_dom_data
                .downcast_element()
                .unwrap()
                .inline_layout_data
                .as_ref()
                .unwrap_or_else(|| {
                    panic!("Tried to render node marked as inline root that does not have an inline layout: {:?}", element);
                });

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

        if should_clip {
            scene.pop_layer();
            CLIP_DEPTH.fetch_sub(1, atomic::Ordering::SeqCst);
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

    fn element_cx<'w>(&'w self, element: &'w Node, location: Point) -> ElementCx<'w> {
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
            list_item: element.element_data().unwrap().list_item_data.as_deref(),
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
    list_item: Option<&'a ListItemLayout>,
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
        let Some(svg) = self.svg else {
            return;
        };

        let width = self.frame.inner_rect.width() as u32;
        let height = self.frame.inner_rect.height() as u32;
        let svg_size = svg.size();

        let x_scale = width as f64 / svg_size.width() as f64;
        let y_scale = height as f64 / svg_size.height() as f64;

        let transform = Affine::translate((self.pos.x * self.scale, self.pos.y * self.scale))
            .pre_scale_non_uniform(x_scale, y_scale);

        let fragment = vello_svg::render_tree(svg);
        scene.append(&fragment, Some(transform));
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

        // Draw background color (if any)
        self.draw_solid_frame(scene);
        let segments = &self.style.get_background().background_image.0;
        for segment in segments.iter().rev() {
            match segment {
                None => {
                    // Do nothing
                }
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
                flags,
                // compat_mode,
                ..
            } => self.draw_linear_gradient(scene, direction, items, *flags),
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
        flags: GradientFlags,
    ) {
        let bb = self.frame.outer_rect.bounding_box();

        let shape = self.frame.frame();
        let center = bb.center();
        let rect = self.frame.inner_rect;
        let (start, end) = match direction {
            LineDirection::Angle(angle) => {
                let angle = -angle.radians64() + std::f64::consts::PI;
                let offset_length = rect.width() / 2.0 * angle.sin().abs()
                    + rect.height() / 2.0 * angle.cos().abs();
                let offset_vec = Vec2::new(angle.sin(), angle.cos()) * offset_length;
                (center - offset_vec, center + offset_vec)
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

        let gradient_length = CSSPixelLength::new((start.distance(end) / self.scale) as f32);
        let repeating = flags.contains(GradientFlags::REPEATING);

        let mut gradient = peniko::Gradient::new_linear(start, end).with_extend(if repeating {
            peniko::Extend::Repeat
        } else {
            peniko::Extend::Pad
        });

        let (first_offset, last_offset) =
            Self::resolve_length_color_stops(items, gradient_length, &mut gradient, repeating);
        if repeating && gradient.stops.len() > 1 {
            gradient.kind = peniko::GradientKind::Linear {
                start: start + (end - start) * first_offset as f64,
                end: end + (start - end) * (1.0 - last_offset) as f64,
            };
        }
        let brush = peniko::BrushRef::Gradient(&gradient);
        scene.fill(peniko::Fill::NonZero, self.transform, brush, None, &shape);
    }

    #[inline]
    fn resolve_color_stops<T>(
        items: &OwnedSlice<GenericGradientItem<GenericColor<Percentage>, T>>,
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
                    (color.as_vello(), step * idx as f32)
                }
                GenericGradientItem::ComplexColorStop { color, position } => {
                    let offset = item_resolver(gradient_length, position);
                    if let Some(offset) = offset {
                        (color.as_vello(), offset)
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
                            let color = Color::rgba8(
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
                            gradient.stops.push(peniko::ColorStop {
                                color,
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
                for stop in &mut gradient.stops {
                    stop.offset = (stop.offset - first_offset) * scale_inv;
                }
            }
            (first_offset, last_offset)
        } else {
            (0.0, 1.0)
        }
    }

    #[inline]
    fn resolve_length_color_stops(
        items: &OwnedSlice<GenericGradientItem<GenericColor<Percentage>, LengthPercentage>>,
        gradient_length: CSSPixelLength,
        gradient: &mut Gradient,
        repeating: bool,
    ) -> (f32, f32) {
        Self::resolve_color_stops(
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
    fn resolve_angle_color_stops(
        items: &OwnedSlice<GenericGradientItem<GenericColor<Percentage>, AngleOrPercentage>>,
        gradient_length: CSSPixelLength,
        gradient: &mut Gradient,
        repeating: bool,
    ) -> (f32, f32) {
        Self::resolve_color_stops(
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

    // fn draw_image_frame(&self, scene: &mut Scene) {}

    fn draw_outset_box_shadow(&self, scene: &mut Scene) {
        let box_shadow = &self.style.get_effects().box_shadow.0;

        // TODO: Only apply clip if element has transparency
        let has_outset_shadow = box_shadow.iter().any(|s| !s.inset);
        if has_outset_shadow {
            CLIPS_WANTED.fetch_add(1, atomic::Ordering::SeqCst);
            let clips_available = CLIPS_USED.load(atomic::Ordering::SeqCst) <= CLIP_LIMIT;
            if clips_available {
                scene.push_layer(Mix::Clip, 1.0, self.transform, &self.frame.shadow_clip());
                CLIPS_USED.fetch_add(1, atomic::Ordering::SeqCst);
                let depth = CLIP_DEPTH.fetch_add(1, atomic::Ordering::SeqCst) + 1;
                CLIP_DEPTH_USED.fetch_max(depth, atomic::Ordering::SeqCst);
            }
        }

        for shadow in box_shadow.iter().filter(|s| !s.inset) {
            let shadow_color = shadow.base.color.as_vello();
            if shadow_color != Color::TRANSPARENT {
                let transform = self.transform.then_translate(Vec2 {
                    x: shadow.base.horizontal.px() as f64,
                    y: shadow.base.vertical.px() as f64,
                });

                //TODO draw shadows with matching individual radii instead of averaging
                let radius = (self.frame.border_top_left_radius_height
                    + self.frame.border_bottom_left_radius_width
                    + self.frame.border_bottom_left_radius_height
                    + self.frame.border_bottom_left_radius_width
                    + self.frame.border_bottom_right_radius_height
                    + self.frame.border_bottom_right_radius_width
                    + self.frame.border_top_right_radius_height
                    + self.frame.border_top_right_radius_width)
                    / 8.0;

                // Fill the color
                scene.draw_blurred_rounded_rect(
                    transform,
                    self.frame.outer_rect,
                    shadow_color,
                    radius,
                    shadow.base.blur.px() as f64,
                );
            }
        }

        if has_outset_shadow {
            scene.pop_layer();
            CLIP_DEPTH.fetch_sub(1, atomic::Ordering::SeqCst);
        }
    }

    fn draw_inset_box_shadow(&self, scene: &mut Scene) {
        let box_shadow = &self.style.get_effects().box_shadow.0;
        let has_inset_shadow = box_shadow.iter().any(|s| s.inset);
        if has_inset_shadow {
            CLIPS_WANTED.fetch_add(1, atomic::Ordering::SeqCst);
            let clips_available = CLIPS_USED.load(atomic::Ordering::SeqCst) <= CLIP_LIMIT;
            if clips_available {
                scene.push_layer(Mix::Clip, 1.0, self.transform, &self.frame.frame());
                CLIPS_USED.fetch_add(1, atomic::Ordering::SeqCst);
                let depth = CLIP_DEPTH.fetch_add(1, atomic::Ordering::SeqCst) + 1;
                CLIP_DEPTH_USED.fetch_max(depth, atomic::Ordering::SeqCst);
            }
        }
        for shadow in box_shadow.iter().filter(|s| s.inset) {
            let shadow_color = shadow.base.color.as_vello();
            if shadow_color != Color::TRANSPARENT {
                let transform = self.transform.then_translate(Vec2 {
                    x: shadow.base.horizontal.px() as f64,
                    y: shadow.base.vertical.px() as f64,
                });

                //TODO draw shadows with matching individual radii instead of averaging
                let radius = (self.frame.border_top_left_radius_height
                    + self.frame.border_bottom_left_radius_width
                    + self.frame.border_bottom_left_radius_height
                    + self.frame.border_bottom_left_radius_width
                    + self.frame.border_bottom_right_radius_height
                    + self.frame.border_bottom_right_radius_width
                    + self.frame.border_top_right_radius_height
                    + self.frame.border_top_right_radius_width)
                    / 8.0;

                // Fill the color
                scene.draw_blurred_rounded_rect(
                    transform,
                    self.frame.outer_rect,
                    shadow_color,
                    radius,
                    shadow.base.blur.px() as f64,
                );
            }
        }
        if has_inset_shadow {
            scene.pop_layer();
            CLIP_DEPTH.fetch_sub(1, atomic::Ordering::SeqCst);
        }
    }

    fn draw_solid_frame(&self, scene: &mut Scene) {
        let background_color = &self.style.get_background().background_color;
        let bg_color = background_color.as_vello();

        if bg_color != Color::TRANSPARENT {
            let shape = self.frame.frame();

            // Fill the color
            scene.fill(Fill::NonZero, self.transform, bg_color, None, &shape);
        }
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
        scene: &mut Scene,
        shape: &EndingShape<NonNegative<CSSPixelLength>, NonNegative<LengthPercentage>>,
        position: &GenericPosition<LengthPercentage, LengthPercentage>,
        items: &OwnedSlice<GenericGradientItem<GenericColor<Percentage>, LengthPercentage>>,
        flags: GradientFlags,
    ) {
        let bez_path = self.frame.frame();
        let rect = self.frame.inner_rect;
        let repeating = flags.contains(GradientFlags::REPEATING);

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
                        .then_translate(self.get_translation(position, rect)),
                )
            } else {
                None
            }
        };

        let brush = peniko::BrushRef::Gradient(&gradient);
        scene.fill(
            peniko::Fill::NonZero,
            self.transform,
            brush,
            gradient_transform,
            &bez_path,
        );
    }

    fn draw_conic_gradient(
        &self,
        scene: &mut Scene,
        angle: &Angle,
        position: &GenericPosition<LengthPercentage, LengthPercentage>,
        items: &OwnedSlice<GenericGradientItem<GenericColor<Percentage>, AngleOrPercentage>>,
        flags: GradientFlags,
    ) {
        let bez_path = self.frame.frame();
        let rect = self.frame.inner_rect;

        let repeating = flags.contains(GradientFlags::REPEATING);
        let mut gradient = peniko::Gradient::new_sweep((0.0, 0.0), 0.0, std::f32::consts::PI * 2.0)
            .with_extend(if repeating {
                peniko::Extend::Repeat
            } else {
                peniko::Extend::Pad
            });

        let (first_offset, last_offset) = Self::resolve_angle_color_stops(
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

        let brush = peniko::BrushRef::Gradient(&gradient);

        scene.fill(
            peniko::Fill::NonZero,
            self.transform,
            brush,
            Some(
                Affine::rotate(angle.radians() as f64 - std::f64::consts::PI / 2.0)
                    .then_translate(self.get_translation(position, rect)),
            ),
            &bez_path,
        );
    }

    #[inline]
    fn get_translation(
        &self,
        position: &GenericPosition<LengthPercentage, LengthPercentage>,
        rect: Rect,
    ) -> Vec2 {
        Vec2::new(
            self.frame.inner_rect.x0
                + position
                    .horizontal
                    .resolve(CSSPixelLength::new(rect.width() as f32))
                    .px() as f64,
            self.frame.inner_rect.y0
                + position
                    .vertical
                    .resolve(CSSPixelLength::new(rect.height() as f32))
                    .px() as f64,
        )
    }

    fn draw_input(&self, scene: &mut Scene) {
        if self.element.local_name() == "input"
            && matches!(self.element.attr(local_name!("type")), Some("checkbox"))
        {
            let Some(checked) = self
                .element
                .element_data()
                .and_then(|data| data.checkbox_input_checked())
            else {
                return;
            };
            let disabled = self.element.attr(local_name!("disabled")).is_some();

            // TODO this should be coming from css accent-color, but I couldn't find how to retrieve it
            let accent_color = if disabled {
                peniko::Color {
                    r: 209,
                    g: 209,
                    b: 209,
                    a: 255,
                }
            } else {
                self.style.get_inherited_text().color.as_vello()
            };

            let scale = (self
                .frame
                .outer_rect
                .width()
                .min(self.frame.outer_rect.height())
                - 4.0)
                .max(0.0)
                / 16.0;

            let frame = self.frame.outer_rect.to_rounded_rect(scale * 2.0);

            if checked {
                scene.fill(Fill::NonZero, self.transform, accent_color, None, &frame);

                //Tick code derived from masonry
                let mut path = BezPath::new();
                path.move_to((2.0, 9.0));
                path.line_to((6.0, 13.0));
                path.line_to((14.0, 2.0));

                path.apply_affine(Affine::translate(Vec2 { x: 2.0, y: 1.0 }).then_scale(scale));

                let style = Stroke {
                    width: 2.0 * scale,
                    join: Join::Round,
                    miter_limit: 10.0,
                    start_cap: Cap::Round,
                    end_cap: Cap::Round,
                    dash_pattern: Default::default(),
                    dash_offset: 0.0,
                };

                scene.stroke(&style, self.transform, Color::WHITE, None, &path);
            } else {
                scene.fill(Fill::NonZero, self.transform, Color::WHITE, None, &frame);
                scene.stroke(
                    &Stroke::default(),
                    self.transform,
                    accent_color,
                    None,
                    &frame,
                );
            }
        }
    }
}
