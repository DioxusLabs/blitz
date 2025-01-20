use std::sync::atomic::{self, AtomicUsize};
use std::sync::Arc;

use super::multicolor_rounded_rect::{Edge, ElementFrame};
use crate::util::{Color, ToColorColor};
use blitz_dom::node::{
    ImageData, ListItemLayout, ListItemLayoutPosition, Marker, NodeData, RasterImageData,
    TextBrush, TextInputData, TextNodeData,
};
use blitz_dom::{local_name, BaseDocument, ElementNodeData, Node};
use blitz_traits::Devtools;

use color::DynamicColor;
use euclid::Transform3D;
use parley::Line;
use style::color::AbsoluteColor;
use style::{
    dom::TElement,
    properties::{
        generated::longhands::visibility::computed_value::T as StyloVisibility,
        style_structs::{Font, Outline},
        ComputedValues,
    },
    values::{
        computed::{
            Angle, AngleOrPercentage, CSSPixelLength, Gradient as StyloGradient, LengthPercentage,
            LineDirection, Overflow, Percentage,
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

use image::imageops::FilterType;
use parley::layout::PositionedLayoutItem;
use style::values::generics::color::GenericColor;
use style::values::generics::image::{
    GenericCircle, GenericEllipse, GenericEndingShape, ShapeExtent,
};
use style::values::specified::percentage::ToPercentage;
use taffy::Layout;
use vello::kurbo::{self, BezPath, Cap, Circle, Join};
use vello::peniko::Gradient;
use vello::{
    kurbo::{Affine, Point, Rect, Shape, Stroke, Vec2},
    peniko::{self, Fill, Mix},
    Scene,
};
#[cfg(feature = "svg")]
use vello_svg::usvg;

type GradientItem<T> = GenericGradientItem<GenericColor<Percentage>, T>;

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
    dom: &BaseDocument,
    scale: f64,
    width: u32,
    height: u32,
    devtool_config: Devtools,
) {
    CLIPS_USED.store(0, atomic::Ordering::SeqCst);
    CLIPS_WANTED.store(0, atomic::Ordering::SeqCst);

    let generator = VelloSceneGenerator {
        dom,
        scale,
        width,
        height,
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
    dom: &'dom BaseDocument,
    scale: f64,
    width: u32,
    height: u32,
    devtools: Devtools,
}

impl VelloSceneGenerator<'_> {
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

        let root_element = self.dom.as_ref().root_element();
        let root_id = root_element.id;
        let bg_width = (self.width as f32).max(root_element.final_layout.size.width);
        let bg_height = (self.height as f32).max(root_element.final_layout.size.height);

        let background_color = {
            let html_color = root_element
                .primary_styles()
                .unwrap()
                .clone_background_color();
            if html_color == GenericColor::TRANSPARENT_BLACK {
                root_element
                    .children
                    .iter()
                    .find_map(|id| {
                        self.dom.as_ref().get_node(*id).filter(|node| {
                            node.data
                                .is_element_with_tag_name(&local_name!("body"))
                        })
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
    fn render_debug_overlay(&self, scene: &mut Scene, node_id: usize) {
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
        if node.primary_styles().unwrap().get_effects().opacity == 0.0 {
            return;
        }

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

        let transform = Affine::translate(content_position.to_vec2() * self.scale);
        let origin = kurbo::Point { x: 0.0, y: 0.0 };
        let clip = Rect::from_origin_size(origin, content_box_size);

        // Optimise zero-area (/very small area) clips by not rendering at all
        if should_clip && clip.area() < 0.01 {
            return;
        }

        if should_clip {
            CLIPS_WANTED.fetch_add(1, atomic::Ordering::SeqCst);
        }

        let mut cx = self.element_cx(node, layout, box_position);
        cx.stroke_effects(scene);
        cx.stroke_outline(scene);
        cx.draw_outset_box_shadow(scene);
        cx.draw_background(scene);

        if should_clip && clips_available {
            scene.push_layer(Mix::Clip, 1.0, transform, &cx.frame.frame());
            CLIPS_USED.fetch_add(1, atomic::Ordering::SeqCst);
            let depth = CLIP_DEPTH.fetch_add(1, atomic::Ordering::SeqCst) + 1;
            CLIP_DEPTH_USED.fetch_max(depth, atomic::Ordering::SeqCst);
        }

        cx.draw_inset_box_shadow(scene);
        cx.stroke_border(scene);
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

        if should_clip {
            scene.pop_layer();
            CLIP_DEPTH.fetch_sub(1, atomic::Ordering::SeqCst);
        }
    }

    fn render_node(&self, scene: &mut Scene, node_id: usize, location: Point) {
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
            transform *= kurbo_transform;
        }

        // todo: maybe cache this so we don't need to constantly be figuring it out
        // It is quite a bit of math to calculate during render/traverse
        // Also! we can cache the bezpaths themselves, saving us a bunch of work
        let frame = ElementFrame::new(&style, &layout, scale);

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

#[cfg(feature = "svg")]
fn compute_background_size(
    style: &ComputedValues,
    container_w: f32,
    container_h: f32,
    bg_idx: usize,
    bg_w: f32,
    bg_h: f32,
    scale: f32,
) -> kurbo::Size {
    use style::values::computed::{BackgroundSize, Length};
    use style::values::generics::length::GenericLengthPercentageOrAuto as Lpa;

    let bg_size = style
        .get_background()
        .background_size
        .0
        .get(bg_idx)
        .cloned()
        .unwrap_or(BackgroundSize::auto());

    let (width, height): (f32, f32) = match bg_size {
        BackgroundSize::ExplicitSize { width, height } => {
            let width = width.map(|w| w.0.resolve(Length::new(container_w)));
            let height = height.map(|h| h.0.resolve(Length::new(container_h)));

            match (width, height) {
                (Lpa::LengthPercentage(width), Lpa::LengthPercentage(height)) => {
                    (width.px(), height.px())
                }
                (Lpa::LengthPercentage(width), Lpa::Auto) => {
                    let height = (width.px() / bg_w) * bg_h;
                    (width.px(), height)
                }
                (Lpa::Auto, Lpa::LengthPercentage(height)) => {
                    let width = (height.px() / bg_h) * bg_w;
                    (width, height.px())
                }
                (Lpa::Auto, Lpa::Auto) => (bg_w * scale, bg_h * scale),
            }
        }
        BackgroundSize::Cover => {
            let x_ratio = container_w / bg_w;
            let y_ratio = container_h / bg_h;

            let ratio = if x_ratio < 1.0 || y_ratio < 1.0 {
                x_ratio.min(y_ratio)
            } else {
                x_ratio.max(y_ratio)
            };

            (bg_w * ratio, bg_h * ratio)
        }
        BackgroundSize::Contain => {
            let x_ratio = container_w / bg_w;
            let y_ratio = container_h / bg_h;

            let ratio = if x_ratio < 1.0 || y_ratio < 1.0 {
                x_ratio.max(y_ratio)
            } else {
                x_ratio.min(y_ratio)
            };

            (bg_w * ratio, bg_h * ratio)
        }
    };

    kurbo::Size {
        width: width as f64,
        height: height as f64,
    }
}

/// Ensure that the `resized_image` field has a correctly sized image
fn ensure_resized_image(data: &RasterImageData, width: u32, height: u32) {
    let mut resized_image = data.resized_image.borrow_mut();

    if resized_image.is_none()
        || resized_image
            .as_ref()
            .is_some_and(|img| img.width != width || img.height != height)
    {
        let image_data = data
            .image
            .clone()
            .resize_to_fill(width, height, FilterType::Lanczos3)
            .into_rgba8()
            .into_raw();

        let peniko_image = peniko::Image {
            data: peniko::Blob::new(Arc::new(image_data)),
            format: peniko::ImageFormat::Rgba8,
            width,
            height,
            alpha: 1.0,
            x_extend: peniko::Extend::Pad,
            y_extend: peniko::Extend::Pad,
            quality: peniko::ImageQuality::High,
        };

        *resized_image = Some(Arc::new(peniko_image));
    }
}

/// A context of loaded and hot data to draw the element from
struct ElementCx<'a> {
    context: &'a VelloSceneGenerator<'a>,
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
    fn with_maybe_clip(
        &self,
        scene: &mut Scene,
        mut condition: impl FnMut() -> bool,
        mut cb: impl FnMut(&ElementCx<'_>, &mut Scene),
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

    fn draw_inline_layout(&self, scene: &mut Scene, pos: Point) {
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

    fn draw_text_input_text(&self, scene: &mut Scene, pos: Point) {
        // Render the text in text inputs
        if let Some(input_data) = self.text_input {
            let transform = Affine::translate((pos.x * self.scale, pos.y * self.scale));

            if self.node.is_focussed() {
                // Render selection/caret
                for rect in input_data.editor.selection_geometry().iter() {
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

    fn draw_marker(&self, scene: &mut Scene, pos: Point) {
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

    fn draw_children(&self, scene: &mut Scene) {
        if let Some(children) = &*self.node.paint_children.borrow() {
            for child_id in children {
                self.render_node(scene, *child_id, self.pos);
            }
        }
    }

    fn stroke_text<'a>(
        &self,
        scene: &mut Scene,
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

                    scene
                        .draw_glyphs(font)
                        .brush(&style.brush)
                        .hint(true)
                        .transform(transform)
                        .glyph_transform(glyph_xform)
                        .font_size(font_size)
                        .normalized_coords(run.normalized_coords())
                        .draw(
                            Fill::NonZero,
                            glyph_run.glyphs().map(|glyph| {
                                let gx = x + glyph.x;
                                let gy = y - glyph.y;
                                x += glyph.advance;

                                vello::Glyph {
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
                        scene.stroke(&Stroke::new(size as f64), transform, brush, None, &line)
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
    fn draw_svg(&self, scene: &mut Scene) {
        let Some(svg) = self.svg else {
            return;
        };

        let width = self.frame.inner_rect.width() as u32;
        let height = self.frame.inner_rect.height() as u32;
        let svg_size = svg.size();

        let x_scale = width as f64 / svg_size.width() as f64;
        let y_scale = height as f64 / svg_size.height() as f64;

        let box_inset = self.frame.inner_rect.origin();
        let transform = Affine::translate((
            self.pos.x * self.scale + box_inset.x,
            self.pos.y * self.scale + box_inset.y,
        ))
        .pre_scale_non_uniform(x_scale, y_scale);

        let fragment = vello_svg::render_tree(svg);
        scene.append(&fragment, Some(transform));
    }

    #[cfg(feature = "svg")]
    fn draw_svg_bg_image(&self, scene: &mut Scene, idx: usize) {
        use style::{values::computed::Length, Zero as _};

        let bg_image = self.element.background_images.get(idx);

        let Some(Some(bg_image)) = bg_image.as_ref() else {
            return;
        };
        let ImageData::Svg(svg) = &bg_image.image else {
            return;
        };

        let frame_w = self.frame.inner_rect.width() as f32;
        let frame_h = self.frame.inner_rect.height() as f32;

        let svg_size = svg.size();
        let bg_size = compute_background_size(
            &self.style,
            frame_w,
            frame_h,
            idx,
            svg_size.width(),
            svg_size.height(),
            self.scale as f32,
        );

        let x_ratio = bg_size.width as f64 / svg_size.width() as f64;
        let y_ratio = bg_size.height as f64 / svg_size.height() as f64;

        let bg_pos_x = self
            .style
            .get_background()
            .background_position_x
            .0
            .get(idx)
            .cloned()
            .unwrap_or(LengthPercentage::zero())
            .resolve(Length::new(frame_w - (bg_size.width as f32)))
            .px() as f64;
        let bg_pos_y = self
            .style
            .get_background()
            .background_position_y
            .0
            .get(idx)
            .cloned()
            .unwrap_or(LengthPercentage::zero())
            .resolve(Length::new(frame_h - bg_size.height as f32))
            .px() as f64;

        let transform = Affine::translate((
            (self.pos.x * self.scale) + bg_pos_x,
            (self.pos.y * self.scale) + bg_pos_y,
        ))
        .pre_scale_non_uniform(x_ratio, y_ratio);

        let fragment = vello_svg::render_tree(svg);
        scene.append(&fragment, Some(transform));
    }

    fn draw_image(&self, scene: &mut Scene) {
        let width = self.frame.inner_rect.width() as u32;
        let height = self.frame.inner_rect.height() as u32;

        if let Some(image_data) = self.element.raster_image_data() {
            ensure_resized_image(image_data, width, height);
            let resized_image = image_data.resized_image.borrow();
            scene.draw_image(resized_image.as_ref().unwrap(), self.transform);
        }
    }

    fn draw_raster_bg_image(&self, scene: &mut Scene, idx: usize) {
        let width = self.frame.inner_rect.width() as u32;
        let height = self.frame.inner_rect.height() as u32;

        let bg_image = self.element.background_images.get(idx);

        if let Some(Some(bg_image)) = bg_image.as_ref() {
            if let ImageData::Raster(image_data) = &bg_image.image {
                ensure_resized_image(image_data, width, height);
                let resized_image = image_data.resized_image.borrow();
                scene.draw_image(resized_image.as_ref().unwrap(), self.transform);
            }
        }
    }

    fn stroke_devtools(&self, scene: &mut Scene) {
        if self.devtools.show_layout {
            let shape = &self.frame.outer_rect;
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

    fn draw_background(&self, scene: &mut Scene) {
        use GenericImage::*;

        CLIPS_WANTED.fetch_add(1, atomic::Ordering::SeqCst);
        let clips_available = CLIPS_USED.load(atomic::Ordering::SeqCst) <= CLIP_LIMIT;
        if clips_available {
            scene.push_layer(Mix::Clip, 1.0, self.transform, &self.frame.frame());
            CLIPS_USED.fetch_add(1, atomic::Ordering::SeqCst);
            let depth = CLIP_DEPTH.fetch_add(1, atomic::Ordering::SeqCst) + 1;
            CLIP_DEPTH_USED.fetch_max(depth, atomic::Ordering::SeqCst);
        }

        // Draw background color (if any)
        self.draw_solid_frame(scene);
        let segments = &self.style.get_background().background_image.0;
        for (idx, segment) in segments.iter().enumerate().rev() {
            match segment {
                None => {
                    // Do nothing
                }
                Gradient(gradient) => self.draw_gradient_frame(scene, gradient),
                Url(_) => {
                    self.draw_raster_bg_image(scene, idx);
                    #[cfg(feature = "svg")]
                    self.draw_svg_bg_image(scene, idx);
                }
                PaintWorklet(_) => todo!("Implement background drawing for Image::PaintWorklet"),
                CrossFade(_) => todo!("Implement background drawing for Image::CrossFade"),
                ImageSet(_) => todo!("Implement background drawing for Image::ImageSet"),
            }
        }

        scene.pop_layer();
        CLIP_DEPTH.fetch_sub(1, atomic::Ordering::SeqCst);
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
        items: &[GradientItem<LengthPercentage>],
        flags: GradientFlags,
    ) {
        let bb = self.frame.outer_rect.bounding_box();
        let current_color = self.style.clone_color();

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
        let brush = peniko::BrushRef::Gradient(&gradient);
        scene.fill(peniko::Fill::NonZero, self.transform, brush, None, &shape);
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

    // fn draw_image_frame(&self, scene: &mut Scene) {}

    fn draw_outset_box_shadow(&self, scene: &mut Scene) {
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
                        scene.draw_blurred_rounded_rect(
                            transform,
                            elem_cx.frame.outer_rect,
                            shadow_color,
                            radius,
                            shadow.base.blur.px() as f64,
                        );
                    }
                }
            },
        )
    }

    fn draw_inset_box_shadow(&self, scene: &mut Scene) {
        let box_shadow = &self.style.get_effects().box_shadow.0;
        let current_color = self.style.clone_color();
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
        let current_color = self.style.clone_color();
        let background_color = &self.style.get_background().background_color;
        let bg_color = background_color
            .resolve_to_absolute(&current_color)
            .as_srgb_color();

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
                .outer_rect
                .width()
                .min(self.frame.outer_rect.height())
                - 4.0)
                .max(0.0)
                / 16.0;

            let frame = self.frame.outer_rect.to_rounded_rect(scale * 2.0);

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
    type Target = VelloSceneGenerator<'a>;
    fn deref(&self) -> &Self::Target {
        self.context
    }
}
