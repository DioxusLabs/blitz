mod render_background;

use std::sync::atomic::{self, AtomicUsize};

use super::multicolor_rounded_rect::{Edge, ElementFrame};
use crate::util::{Color, ToColorColor};
use blitz_dom::node::{
    ListItemLayout, ListItemLayoutPosition, Marker, NodeData, RasterImageData, TextBrush,
    TextInputData, TextNodeData,
};
use blitz_dom::{BaseDocument, ElementNodeData, Node, local_name};
use blitz_traits::Devtools;

use euclid::Transform3D;
use parley::Line;
use style::{
    dom::TElement,
    properties::{
        ComputedValues,
        generated::longhands::visibility::computed_value::T as StyloVisibility,
        style_structs::{Font, Outline},
    },
    values::{
        computed::{CSSPixelLength, Overflow},
        specified::{BorderStyle, OutlineStyle},
    },
};

use kurbo::{self, BezPath, Cap, Circle, Join};
use kurbo::{Affine, Point, Rect, Stroke, Vec2};
use parley::layout::PositionedLayoutItem;
use peniko::{self, Fill, Mix};
use style::values::generics::color::GenericColor;
use taffy::Layout;

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
pub fn paint_scene(
    scene: &mut impl anyrender::Scene,
    dom: &BaseDocument,
    scale: f64,
    width: u32,
    height: u32,
) {
    CLIPS_USED.store(0, atomic::Ordering::SeqCst);
    CLIPS_WANTED.store(0, atomic::Ordering::SeqCst);

    let devtools = *dom.devtools();
    let generator = BlitzDomPainter {
        dom,
        scale,
        width,
        height,
        devtools,
    };
    generator.paint_scene(scene);

    // println!(
    //     "Rendered using {} clips (depth: {}) (wanted: {})",
    //     CLIPS_USED.load(atomic::Ordering::SeqCst),
    //     CLIP_DEPTH_USED.load(atomic::Ordering::SeqCst),
    //     CLIPS_WANTED.load(atomic::Ordering::SeqCst)
    // );
}

/// A short-lived struct which holds a bunch of parameters for rendering a scene so
/// that we don't have to pass them down as parameters
pub struct BlitzDomPainter<'dom> {
    /// Input parameters (read only) for generating the Scene
    dom: &'dom BaseDocument,
    scale: f64,
    width: u32,
    height: u32,
    devtools: Devtools,
}

impl BlitzDomPainter<'_> {
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
    pub fn paint_scene(&self, scene: &mut impl anyrender::Scene) {
        // Simply render the document (the root element (note that this is not the same as the root node)))
        scene.reset();
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
    fn render_debug_overlay(&self, scene: &mut impl anyrender::Scene, node_id: usize) {
        let scale = self.scale;

        let viewport_scroll = self.dom.as_ref().viewport_scroll();
        let mut node = &self.dom.as_ref().tree()[node_id];

        let taffy::Layout {
            size,
            border,
            padding,
            margin,
            ..
        } = node.final_layout;
        let taffy::Size { width, height } = size;

        let padding_border = padding + border;
        let scaled_pb = padding_border.map(|v| f64::from(v) * scale);
        let scaled_padding = padding.map(|v| f64::from(v) * scale);
        let scaled_border = border.map(|v| f64::from(v) * scale);
        let scaled_margin = margin.map(|v| f64::from(v) * scale);

        let content_width = width - padding_border.left - padding_border.right;
        let content_height = height - padding_border.top - padding_border.bottom;

        let taffy::Point { x, y } = node.final_layout.location;

        let mut abs_x = x;
        let mut abs_y = y;
        while let Some(parent_id) = node.layout_parent.get() {
            node = &self.dom.as_ref().tree()[parent_id];
            let taffy::Point { x, y } = node.final_layout.location;
            abs_x += x;
            abs_y += y;
        }

        abs_x -= viewport_scroll.x as f32;
        abs_y -= viewport_scroll.y as f32;

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
        let fill_color = Color::from_rgba8(66, 144, 245, 128); // blue
        scene.fill(peniko::Fill::NonZero, transform, fill_color, None, &rect);

        fn draw_cutout_rect(
            scene: &mut impl anyrender::Scene,
            base_translation: Vec2,
            size: Vec2,
            edge_widths: taffy::Rect<f64>,
            color: Color,
        ) {
            let mut fill = |pos: Vec2, width: f64, height: f64| {
                scene.fill(
                    peniko::Fill::NonZero,
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

        let padding_color = Color::from_rgba8(81, 144, 66, 128); // green
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

        let border_color = Color::from_rgba8(245, 66, 66, 128); // red
        draw_cutout_rect(
            scene,
            base_translation,
            Vec2::new(width, height),
            scaled_border.map(f64::from),
            border_color,
        );

        let margin_color = Color::from_rgba8(249, 204, 157, 128); // orange
        draw_cutout_rect(
            scene,
            base_translation - Vec2::new(scaled_margin.left, scaled_margin.top),
            Vec2::new(
                width + scaled_margin.left + scaled_margin.right,
                height + scaled_margin.top + scaled_margin.bottom,
            ),
            scaled_margin.map(f64::from),
            margin_color,
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
    fn render_element(&self, scene: &mut impl anyrender::Scene, node_id: usize, location: Point) {
        let node = &self.dom.as_ref().tree()[node_id];

        // Early return if the element is hidden
        if matches!(node.style.display, taffy::Display::None) {
            return;
        }

        // Only draw elements with a style
        if node.primary_styles().is_none() {
            return;
        }

        // Hide elements with "hidden" attribute
        if let Some("true" | "") = node.attr(local_name!("hidden")) {
            return;
        }

        // Hide inputs with type=hidden
        // Implemented here rather than using the style engine for performance reasons
        if node.local_name() == "input" && node.attr(local_name!("type")) == Some("hidden") {
            return;
        }

        // Hide elements with a visibility style other than visible
        if node
            .primary_styles()
            .unwrap()
            .get_inherited_box()
            .visibility
            != StyloVisibility::Visible
        {
            return;
        }

        // We can't fully support opacity yet, but we can hide elements with opacity 0
        let opacity = node.primary_styles().unwrap().get_effects().opacity;
        if opacity == 0.0 {
            return;
        }
        let has_opacity = opacity < 1.0;

        // TODO: account for overflow_x vs overflow_y
        let styles = &node.primary_styles().unwrap();
        let overflow_x = styles.get_box().overflow_x;
        let overflow_y = styles.get_box().overflow_y;
        let should_clip =
            !matches!(overflow_x, Overflow::Visible) || !matches!(overflow_y, Overflow::Visible);
        let clips_available = CLIPS_USED.load(atomic::Ordering::SeqCst) <= CLIP_LIMIT;

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
            x: box_position.x + scaled_pb.left,
            y: box_position.y + scaled_pb.top,
        };
        let content_box_size = kurbo::Size {
            width: (size.width as f64 - scaled_pb.left - scaled_pb.right) * self.scale,
            height: (size.height as f64 - scaled_pb.top - scaled_pb.bottom) * self.scale,
        };

        // Don't render things that are out of view
        let scaled_y = box_position.y * self.scale;
        let scaled_content_height = content_size.height.max(size.height) as f64 * self.scale;
        if scaled_y > self.height as f64 || scaled_y + scaled_content_height < 0.0 {
            return;
        }

        let origin = kurbo::Point { x: 0.0, y: 0.0 };
        let clip = Rect::from_origin_size(origin, content_box_size);

        // Optimise zero-area (/very small area) clips by not rendering at all
        if should_clip && clip.area() < 0.01 {
            return;
        }

        let wants_layer = should_clip | has_opacity;
        if wants_layer {
            CLIPS_WANTED.fetch_add(1, atomic::Ordering::SeqCst);
        }

        let mut cx = self.element_cx(node, layout, box_position);
        cx.stroke_effects(scene);
        cx.stroke_outline(scene);
        cx.draw_outset_box_shadow(scene);
        cx.draw_background(scene);
        cx.stroke_border(scene);

        if wants_layer && clips_available {
            // TODO: allow layers with opacity to be unclipped (overflow: visible)
            scene.push_layer(Mix::Clip, 1.0, cx.transform, &cx.frame.frame());
            CLIPS_USED.fetch_add(1, atomic::Ordering::SeqCst);
            let depth = CLIP_DEPTH.fetch_add(1, atomic::Ordering::SeqCst) + 1;
            CLIP_DEPTH_USED.fetch_max(depth, atomic::Ordering::SeqCst);
        }

        cx.draw_inset_box_shadow(scene);
        cx.stroke_devtools(scene);

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
        cx.draw_input(scene);

        cx.draw_text_input_text(scene, content_position);
        cx.draw_inline_layout(scene, content_position);
        cx.draw_marker(scene, content_position);
        cx.draw_children(scene);

        if wants_layer && clips_available {
            scene.pop_layer();
            CLIP_DEPTH.fetch_sub(1, atomic::Ordering::SeqCst);
        }
    }

    fn render_node(&self, scene: &mut impl anyrender::Scene, node_id: usize, location: Point) {
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
        let frame = ElementFrame::new(&style, &layout, scale);

        // the bezpaths for every element are (potentially) cached (not yet, tbd)
        // By performing the transform, we prevent the cache from becoming invalid when the page shifts around
        let mut transform = Affine::translate(box_position.to_vec2() * scale);

        // Apply CSS transform property (where transforms are 2d)
        //
        // TODO: Handle hit testing correctly for transformed nodes
        // TODO: Implement nested transforms
        let (t, has_3d) = &style
            .get_box()
            .transform
            .to_transform_3d_matrix(None)
            .unwrap_or((Transform3D::default(), false));
        if !has_3d {
            // See: https://drafts.csswg.org/css-transforms-2/#two-dimensional-subset
            // And https://docs.rs/kurbo/latest/kurbo/struct.Affine.html#method.new
            let kurbo_transform =
                Affine::new([t.m11, t.m12, t.m21, t.m22, t.m41, t.m42].map(|v| v as f64));

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
                    .resolve(CSSPixelLength::new(frame.border_box.width() as f32))
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
            devtools: &self.devtools,
        }
    }
}

/// Ensure that the `resized_image` field has a correctly sized image
fn to_peniko_image(image: &RasterImageData) -> peniko::Image {
    peniko::Image {
        data: peniko::Blob::new(image.data.clone()),
        format: peniko::ImageFormat::Rgba8,
        width: image.width,
        height: image.height,
        alpha: 1.0,
        x_extend: peniko::Extend::Repeat,
        y_extend: peniko::Extend::Repeat,
        quality: peniko::ImageQuality::High,
    }
}

/// A context of loaded and hot data to draw the element from
struct ElementCx<'a> {
    context: &'a BlitzDomPainter<'a>,
    frame: ElementFrame,
    style: style::servo_arc::Arc<ComputedValues>,
    pos: Point,
    scale: f64,
    node: &'a Node,
    element: &'a ElementNodeData,
    transform: Affine,
    #[cfg(feature = "svg")]
    svg: Option<&'a usvg::Tree>,
    text_input: Option<&'a TextInputData>,
    list_item: Option<&'a ListItemLayout>,
    devtools: &'a Devtools,
}

impl ElementCx<'_> {
    fn with_maybe_clip<S: anyrender::Scene, F: FnMut(&ElementCx<'_>, &mut S)>(
        &self,
        scene: &mut S,
        mut condition: impl FnMut() -> bool,
        mut cb: F,
    ) {
        let clip_wanted = condition();
        let mut clips_available = false;
        if clip_wanted {
            CLIPS_WANTED.fetch_add(1, atomic::Ordering::SeqCst);
            clips_available = CLIPS_USED.load(atomic::Ordering::SeqCst) <= CLIP_LIMIT;
        }
        let do_clip = clip_wanted & clips_available;

        if do_clip {
            scene.push_layer(Mix::Clip, 1.0, self.transform, &self.frame.shadow_clip());
            CLIPS_USED.fetch_add(1, atomic::Ordering::SeqCst);
            let depth = CLIP_DEPTH.fetch_add(1, atomic::Ordering::SeqCst) + 1;
            CLIP_DEPTH_USED.fetch_max(depth, atomic::Ordering::SeqCst);
        }

        cb(self, scene);

        if do_clip {
            scene.pop_layer();
            CLIP_DEPTH.fetch_sub(1, atomic::Ordering::SeqCst);
        }
    }

    fn draw_inline_layout(&self, scene: &mut impl anyrender::Scene, pos: Point) {
        if self.node.is_inline_root {
            let text_layout = self.element
                .inline_layout_data
                .as_ref()
                .unwrap_or_else(|| {
                    panic!("Tried to render node marked as inline root that does not have an inline layout: {:?}", self.node);
                });

            // Render text
            self.stroke_text(scene, text_layout.layout.lines(), pos);
        }
    }

    fn draw_text_input_text(&self, scene: &mut impl anyrender::Scene, pos: Point) {
        // Render the text in text inputs
        if let Some(input_data) = self.text_input {
            let transform = Affine::translate((pos.x * self.scale, pos.y * self.scale));

            if self.node.is_focussed() {
                // Render selection/caret
                for (rect, _line_idx) in input_data.editor.selection_geometry().iter() {
                    scene.fill(
                        Fill::NonZero,
                        transform,
                        color::palette::css::STEEL_BLUE,
                        None,
                        &rect,
                    );
                }
                if let Some(cursor) = input_data.editor.cursor_geometry(1.5) {
                    scene.fill(Fill::NonZero, transform, Color::BLACK, None, &cursor);
                };
            }

            // Render text
            self.stroke_text(scene, input_data.editor.try_layout().unwrap().lines(), pos);
        }
    }

    fn draw_marker(&self, scene: &mut impl anyrender::Scene, pos: Point) {
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
            self.stroke_text(scene, layout.lines(), pos);
        }
    }

    fn draw_children(&self, scene: &mut impl anyrender::Scene) {
        if let Some(children) = &*self.node.paint_children.borrow() {
            for child_id in children {
                self.render_node(scene, *child_id, self.pos);
            }
        }
    }

    fn stroke_text<'a>(
        &self,
        scene: &mut impl anyrender::Scene,
        lines: impl Iterator<Item = Line<'a, TextBrush>>,
        pos: Point,
    ) {
        let transform = Affine::translate((pos.x * self.scale, pos.y * self.scale));
        for line in lines {
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

                    scene.draw_glyphs(
                        font,
                        font_size,
                        true, // hint
                        run.normalized_coords(),
                        Fill::NonZero,
                        &style.brush.brush,
                        1.0, // alpha
                        transform,
                        glyph_xform,
                        glyph_run.glyphs().map(|glyph| {
                            let gx = x + glyph.x;
                            let gy = y - glyph.y;
                            x += glyph.advance;

                            anyrender::Glyph {
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
                        let line = kurbo::Line::new((x, y), (x + w, y));
                        scene.stroke(
                            &Stroke::new(size as f64),
                            transform,
                            &brush.brush,
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

    #[cfg(feature = "svg")]
    fn draw_svg(&self, scene: &mut impl anyrender::Scene) {
        let Some(svg) = self.svg else {
            return;
        };

        let width = self.frame.padding_box.width() as u32;
        let height = self.frame.padding_box.height() as u32;
        let svg_size = svg.size();

        let x_scale = width as f64 / svg_size.width() as f64;
        let y_scale = height as f64 / svg_size.height() as f64;

        let box_inset = self.frame.padding_box.origin();
        let transform = Affine::translate((
            self.pos.x * self.scale + box_inset.x,
            self.pos.y * self.scale + box_inset.y,
        ))
        .pre_scale_non_uniform(x_scale, y_scale);

        anyrender_svg::append_tree(scene, svg, transform);
    }

    fn draw_image(&self, scene: &mut impl anyrender::Scene) {
        if let Some(image) = self.element.raster_image_data() {
            let width = self.frame.content_box.width() as u32;
            let height = self.frame.content_box.height() as u32;
            let x = self.frame.content_box.origin().x;
            let y = self.frame.content_box.origin().y;

            let x_scale = width as f64 / image.width as f64;
            let y_scale = height as f64 / image.height as f64;
            let transform = self
                .transform
                .pre_scale_non_uniform(x_scale, y_scale)
                .then_translate(Vec2 { x, y });

            scene.draw_image(&to_peniko_image(image), transform);
        }
    }

    fn stroke_devtools(&self, scene: &mut impl anyrender::Scene) {
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

        // if self.devtools.show_style {
        //     self.frame.draw_style(scene);
        // }

        // if self.devtools.print_hover {
        //     self.frame.draw_hover(scene);
        // }
    }

    // fn draw_image_frame(&self, scene: &mut impl anyrender::Scene) {}

    fn draw_outset_box_shadow(&self, scene: &mut impl anyrender::Scene) {
        let box_shadow = &self.style.get_effects().box_shadow.0;
        let current_color = self.style.clone_color();

        // TODO: Only apply clip if element has transparency
        let has_outset_shadow = box_shadow.iter().any(|s| !s.inset);
        self.with_maybe_clip(
            scene,
            || has_outset_shadow,
            |elem_cx, scene| {
                for shadow in box_shadow.iter().filter(|s| !s.inset) {
                    let shadow_color = shadow
                        .base
                        .color
                        .resolve_to_absolute(&current_color)
                        .as_srgb_color();
                    if shadow_color != Color::TRANSPARENT {
                        let transform = elem_cx.transform.then_translate(Vec2 {
                            x: shadow.base.horizontal.px() as f64,
                            y: shadow.base.vertical.px() as f64,
                        });

                        //TODO draw shadows with matching individual radii instead of averaging
                        let radius = (elem_cx.frame.border_top_left_radius_height
                            + elem_cx.frame.border_bottom_left_radius_width
                            + elem_cx.frame.border_bottom_left_radius_height
                            + elem_cx.frame.border_bottom_left_radius_width
                            + elem_cx.frame.border_bottom_right_radius_height
                            + elem_cx.frame.border_bottom_right_radius_width
                            + elem_cx.frame.border_top_right_radius_height
                            + elem_cx.frame.border_top_right_radius_width)
                            / 8.0;

                        // Fill the color
                        scene.draw_box_shadow(
                            transform,
                            elem_cx.frame.border_box,
                            shadow_color,
                            radius,
                            shadow.base.blur.px() as f64,
                        );
                    }
                }
            },
        )
    }

    fn draw_inset_box_shadow(&self, scene: &mut impl anyrender::Scene) {
        let box_shadow = &self.style.get_effects().box_shadow.0;
        let current_color = self.style.clone_color();
        let has_inset_shadow = box_shadow.iter().any(|s| s.inset);
        let clips_available = CLIPS_USED.load(atomic::Ordering::SeqCst) <= CLIP_LIMIT;
        if has_inset_shadow {
            CLIPS_WANTED.fetch_add(1, atomic::Ordering::SeqCst);
            if clips_available {
                scene.push_layer(Mix::Clip, 1.0, self.transform, &self.frame.frame());
                CLIPS_USED.fetch_add(1, atomic::Ordering::SeqCst);
                let depth = CLIP_DEPTH.fetch_add(1, atomic::Ordering::SeqCst) + 1;
                CLIP_DEPTH_USED.fetch_max(depth, atomic::Ordering::SeqCst);
            }
        }
        for shadow in box_shadow.iter().filter(|s| s.inset) {
            let shadow_color = shadow
                .base
                .color
                .resolve_to_absolute(&current_color)
                .as_srgb_color();
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
                scene.draw_box_shadow(
                    transform,
                    self.frame.border_box,
                    shadow_color,
                    radius,
                    shadow.base.blur.px() as f64,
                );
            }
        }
        if has_inset_shadow && clips_available {
            scene.pop_layer();
            CLIP_DEPTH.fetch_sub(1, atomic::Ordering::SeqCst);
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
    fn stroke_border(&self, sb: &mut impl anyrender::Scene) {
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
    fn stroke_border_edge(&self, sb: &mut impl anyrender::Scene, edge: Edge) {
        let style = &*self.style;
        let border = style.get_border();
        let path = self.frame.border(edge);

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
            sb.fill(Fill::NonZero, self.transform, color, None, &path);
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
    fn stroke_outline(&self, scene: &mut impl anyrender::Scene) {
        let Outline {
            outline_color,
            outline_style,
            ..
        } = self.style.get_outline();

        let current_color = self.style.clone_color();
        let color = outline_color
            .resolve_to_absolute(&current_color)
            .as_srgb_color();

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
    /// ❌ clip: The clip computed value.
    /// ❌ filter: The filter computed value.
    /// ❌ mix_blend_mode: The mix-blend-mode computed value.
    fn stroke_effects(&self, _scene: &mut impl anyrender::Scene) {
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

    fn draw_input(&self, scene: &mut impl anyrender::Scene) {
        if self.node.local_name() == "input" {
            let Some(checked) = self.element.checkbox_input_checked() else {
                return;
            };
            let disabled = self.node.attr(local_name!("disabled")).is_some();

            // TODO this should be coming from css accent-color, but I couldn't find how to retrieve it
            let accent_color = if disabled {
                Color::from_rgba8(209, 209, 209, 255)
            } else {
                self.style.clone_color().as_srgb_color()
            };

            let scale = (self
                .frame
                .border_box
                .width()
                .min(self.frame.border_box.height())
                - 4.0)
                .max(0.0)
                / 16.0;

            let frame = self.frame.border_box.to_rounded_rect(scale * 2.0);

            let attr_type = self.node.attr(local_name!("type"));

            if attr_type == Some("checkbox") {
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
            } else if attr_type == Some("radio") {
                let center = frame.center();
                let outer_ring = Circle::new(center, 8.0 * scale);
                let gap = Circle::new(center, 6.0 * scale);
                let inner_circle = Circle::new(center, 4.0 * scale);

                if checked {
                    scene.fill(
                        Fill::NonZero,
                        self.transform,
                        accent_color,
                        None,
                        &outer_ring,
                    );
                    scene.fill(Fill::NonZero, self.transform, Color::WHITE, None, &gap);
                    scene.fill(
                        Fill::NonZero,
                        self.transform,
                        accent_color,
                        None,
                        &inner_circle,
                    );
                } else {
                    scene.fill(
                        Fill::NonZero,
                        self.transform,
                        color::palette::css::GRAY,
                        None,
                        &outer_ring,
                    );
                    scene.fill(Fill::NonZero, self.transform, Color::WHITE, None, &gap);
                }
            }
        }
    }
}
impl<'a> std::ops::Deref for ElementCx<'a> {
    type Target = BlitzDomPainter<'a>;
    fn deref(&self) -> &Self::Target {
        self.context
    }
}
