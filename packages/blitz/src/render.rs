mod multicolor_rounded_rect;

use std::num::NonZeroUsize;
use std::sync::Arc;
// So many imports
use self::multicolor_rounded_rect::{Edge, ElementFrame};
use crate::{
    devtools::Devtools,
    fontcache::FontCache,
    imagecache::ImageCache,
    text::TextContext,
    util::{GradientSlice, StyloGradient, ToVelloColor},
    viewport::Viewport,
};
use blitz_dom::{Document, Node};
use html5ever::local_name;
use style::values::specified::position::HorizontalPositionKeyword;
use style::{
    properties::{style_structs::Outline, ComputedValues},
    values::{
        computed::{
            Angle, AngleOrPercentage, CSSPixelLength, LengthPercentage, LineDirection, Percentage,
        },
        generics::{
            color::Color as StyloColor,
            image::{EndingShape, GenericGradient, GenericGradientItem, GenericImage},
            position::GenericPosition,
            NonNegative,
        },
        specified::{position::VerticalPositionKeyword, BorderStyle, OutlineStyle},
    },
    OwnedSlice,
};
use taffy::prelude::Layout;
use vello::{
    kurbo::{Affine, Point, Rect, Shape, Stroke, Vec2},
    peniko::{self, Color, Fill},
    util::RenderContext,
    util::RenderSurface,
    AaSupport, RenderParams, Renderer as VelloRenderer, RendererOptions, Scene,
};
use wgpu::{PresentMode, WasmNotSend};

// Simple struct to hold the state of the renderer
pub struct ActiveRenderState<'s, W> {
    // The fields MUST be in this order, so that the surface is dropped before the window
    renderer: VelloRenderer,
    surface: RenderSurface<'s>,
    pub window: Arc<W>,

    /// The actual viewport of the page that we're getting a glimpse of.
    /// We need this since the part of the page that's being viewed might not be the page in its entirety.
    /// This will let us opt of rendering some stuff
    viewport: Viewport,
}

pub enum RenderState<'s, W> {
    Active(ActiveRenderState<'s, W>),
    // Cache a window so that it can be reused when the app is resumed after being suspended
    Suspended(Option<(Arc<W>, Viewport)>),
}

pub struct Renderer<'s, W> {
    pub dom: Document,

    pub render_state: RenderState<'s, W>,

    pub(crate) render_context: RenderContext,

    /// Our text stencil to be used with vello
    pub(crate) text_context: TextContext,

    /// Our image cache
    pub(crate) images: ImageCache,

    /// A storage of fonts to load in and out.
    /// Whenever we encounter new fonts during parsing + mutations, this will become populated
    pub(crate) fonts: FontCache,

    pub devtools: Devtools,

    hover_node_id: Option<usize>,
}

impl<'a, W> Renderer<'a, W>
where
    W: raw_window_handle::HasWindowHandle
        + raw_window_handle::HasDisplayHandle
        + Sync
        + WasmNotSend
        + 'a,
{
    pub fn new(dom: Document) -> Self {
        // 1. Set up renderer-specific stuff
        // We build an independent viewport which can be dynamically set later
        // The intention here is to split the rendering pipeline away from tao/windowing for rendering to images

        // 2. Set up Vello specific stuff
        let mut render_context = RenderContext::new().unwrap();

        Self {
            render_context,
            render_state: RenderState::Suspended(None),
            dom,
            text_context: Default::default(),
            images: Default::default(),
            fonts: Default::default(),
            devtools: Default::default(),
            hover_node_id: Default::default(),
        }
    }

    pub async fn resume(&mut self, window_builder: impl FnOnce() -> (Arc<W>, Viewport)) {
        let RenderState::Suspended(cached_window) = &mut self.render_state else {
            return;
        };

        let (window, viewport) = cached_window.take().unwrap_or_else(|| window_builder());

        let device = viewport.make_device();
        self.dom.set_stylist_device(device);

        let surface = self
            .render_context
            .create_surface(
                window.clone(),
                viewport.window_size.0,
                viewport.window_size.1,
                PresentMode::AutoVsync,
            )
            .await
            .expect("Error creating surface");

        let default_threads = || -> Option<NonZeroUsize> {
            #[cfg(target_arch = "macos")]
            {
                Some(NonZeroUsize::new(1)?)
            }
            None
        };

        let options = RendererOptions {
            surface_format: Some(surface.config.format),
            antialiasing_support: AaSupport::all(),
            use_cpu: false,
            num_init_threads: default_threads(),
        };

        let renderer =
            VelloRenderer::new(&self.render_context.devices[surface.dev_id].device, options)
                .unwrap();

        self.render_state = RenderState::Active(ActiveRenderState {
            renderer,
            surface,
            window,
            viewport,
        });

        self.dom.resolve();
    }

    pub fn suspend(&mut self) {
        let old_state = std::mem::replace(&mut self.render_state, RenderState::Suspended(None));
        self.render_state = match old_state {
            RenderState::Active(state) => {
                RenderState::Suspended(Some((state.window, state.viewport)))
            }
            RenderState::Suspended(_) => old_state,
        };
    }

    pub fn zoom(&mut self, zoom: f32) {
        let RenderState::Active(state) = &mut self.render_state else {
            return;
        };
        *state.viewport.zoom_mut() += zoom;
        self.kick_viewport()
    }

    pub fn reset_zoom(&mut self) {
        let RenderState::Active(state) = &mut self.render_state else {
            return;
        };
        *state.viewport.zoom_mut() = 1.0;
        self.kick_viewport()
    }

    pub fn mouse_move(&mut self, x: f32, y: f32) -> bool {
        let old_id = self.hover_node_id;
        self.hover_node_id = self.dom.hit(x, y);
        if old_id != self.hover_node_id {
            // println!("Hovered node: {:?}", self.hover_node_id);
            self.devtools.highlight_hover
        } else {
            false
        }
    }

    pub fn click(&mut self) {
        if self.devtools.highlight_hover {
            if let Some(node_id) = self.hover_node_id {
                let node = &self.dom.tree()[node_id];
                println!("Node {}", node.id);
                dbg!(&node.final_layout);
                dbg!(&node.style);

                let children: Vec<_> = node
                    .children
                    .iter()
                    .map(|id| &self.dom.tree()[*id])
                    .map(|node| (node.id, node.order(), node.node_debug_str()))
                    .collect();

                println!("Children: {:?}", children);
                // taffy::print_tree(&self.dom, node_id.into());
            }
        }
    }

    pub fn print_taffy_tree(&self) {
        taffy::print_tree(&self.dom, taffy::NodeId::from(0usize));
    }

    // Adjust the viewport
    pub fn set_size(&mut self, physical_size: (u32, u32)) {
        let RenderState::Active(state) = &mut self.render_state else {
            return;
        };
        state.viewport.window_size = physical_size;
        self.kick_viewport()
    }

    pub fn kick_viewport(&mut self) {
        let RenderState::Active(state) = &mut self.render_state else {
            return;
        };

        let (width, height) = state.viewport.window_size;

        if width > 0 && height > 0 {
            self.dom
                .set_stylist_device(dbg!(state.viewport.make_device()));
            dbg!(&state.viewport);
            self.render_context
                .resize_surface(&mut state.surface, width, height);
        }
    }

    /// Draw the current tree to current render surface
    /// Eventually we'll want the surface itself to be passed into the render function, along with things like the viewport
    ///
    /// This assumes styles are resolved and layout is complete.
    /// Make sure you do those before trying to render
    pub fn render(&mut self, scene: &mut Scene) {
        // Simply render the document (the root element (note that this is not the same as the root node)))
        scene.reset();
        self.render_element(scene, self.dom.root_element().id, Point::ZERO);

        // Render debug overlay
        if self.devtools.highlight_hover {
            if let Some(node_id) = self.hover_node_id {
                self.render_debug_overlay(scene, node_id);
            }
        }

        let RenderState::Active(state) = &mut self.render_state else {
            return;
        };

        let surface_texture = state
            .surface
            .surface
            .get_current_texture()
            .expect("failed to get surface texture");

        let device = &self.render_context.devices[state.surface.dev_id];

        let render_params = RenderParams {
            base_color: Color::WHITE,
            width: state.surface.config.width,
            height: state.surface.config.height,
            antialiasing_method: vello::AaConfig::Msaa16,
        };

        state
            .renderer
            .render_to_surface(
                &device.device,
                &device.queue,
                &scene,
                &surface_texture,
                &render_params,
            )
            .expect("failed to render to surface");

        surface_texture.present();
        device.device.poll(wgpu::Maintain::Wait);
    }

    /// Renders a layout debugging overlay which visualises the content size, padding and border
    /// of the node with a transparent overlay.
    fn render_debug_overlay(&self, scene: &mut Scene, node_id: usize) {
        let RenderState::Active(state) = &self.render_state else {
            return;
        };
        let scale = state.viewport.scale_f64();

        let mut node = &self.dom.tree()[node_id];

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
            node = &self.dom.tree()[parent_id];
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
            base_translation + Vec2::new(scaled_border.left as f64, scaled_border.top as f64),
            Vec2::new(
                content_width as f64 + scaled_padding.left + scaled_padding.right,
                content_height as f64 + scaled_padding.top + scaled_padding.bottom,
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
    fn render_element(&self, scene: &mut Scene, node: usize, location: Point) {
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
        use markup5ever_rcdom::NodeData;

        let element = &self.dom.tree()[node];

        // Early return if the element is hidden
        if matches!(element.style.display, taffy::prelude::Display::None) {
            return;
        }

        let NodeData::Element { name, attrs, .. } = &element.node.data else {
            return;
        };

        // Only draw elements with a style
        if element.data.borrow().styles.get_primary().is_none() {
            return;
        }

        // Hide hidden things...
        // todo: move this to state on the element itself
        if let Some(attr) = attrs
            .borrow()
            .iter()
            .find(|attr| attr.name.local == local_name!("hidden"))
        {
            if attr.value.as_ref() == "true" || attr.value.as_ref() == "" {
                return;
            }
        }

        // Hide inputs with type=hidden
        // Can this just be css?
        if name.local == local_name!("input") {
            if let Some(attr) = attrs
                .borrow()
                .iter()
                .find(|attr| attr.name.local == local_name!("type"))
            {
                if attr.value.as_ref() == "hidden" {
                    return;
                }
            }
        }

        let cx = self.element_cx(element, location);
        cx.stroke_effects(scene);
        cx.stroke_outline(scene);
        cx.stroke_frame(scene);
        cx.stroke_border(scene);
        cx.stroke_devtools(scene);

        for child in &cx.element.children {
            match &self.dom.tree()[*child].node.data {
                NodeData::Element { .. } => self.render_element(scene, *child, cx.pos),
                NodeData::Text { contents } => {
                    let (_layout, pos) = self.node_position(*child, cx.pos);
                    cx.stroke_text(scene, &self.text_context, contents.borrow().as_ref(), pos)
                }
                NodeData::Document => {}
                NodeData::Doctype { .. } => {}
                NodeData::Comment { .. } => {}
                NodeData::ProcessingInstruction { .. } => {}
            }
        }
    }

    fn element_cx<'w>(&'w self, element: &'w Node, location: Point) -> ElementCx {
        let RenderState::Active(state) = &self.render_state else {
            panic!("Renderer is not active");
        };

        let style = element.data.borrow().styles.primary().clone();

        let (layout, pos) = self.node_position(element.id, location);
        let scale = state.viewport.scale_f64();

        let inherited_text = style.get_inherited_text();
        let font = style.get_font();
        let font_size = font.font_size.computed_size().px() as f32;
        let text_color = inherited_text.clone_color().as_vello();

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
            layout,
            pos,
            element,
            font_size,
            text_color,
            transform,
            devtools: &self.devtools,
        }
    }

    fn node_position(&self, node: usize, location: Point) -> (Layout, Point) {
        let layout = self.layout(node);
        let pos = location + Vec2::new(layout.location.x as f64, layout.location.y as f64);
        (layout, pos)
    }

    fn layout(&self, child: usize) -> Layout {
        self.dom.tree()[child].unrounded_layout
        // self.dom.tree()[child].final_layout
    }
}

/// A context of loaded and hot data to draw the element from
struct ElementCx<'a> {
    frame: ElementFrame,
    style: style::servo_arc::Arc<ComputedValues>,
    layout: Layout,
    pos: Point,
    scale: f64,
    element: &'a Node,
    font_size: f32,
    text_color: Color,
    transform: Affine,
    devtools: &'a Devtools,
}

impl ElementCx<'_> {
    fn stroke_text(
        &self,
        scene: &mut Scene,
        text_context: &TextContext,
        contents: &str,
        pos: Point,
    ) {
        let transform = Affine::translate((pos.x * self.scale, pos.y * self.scale))
            .then_translate((0.0, self.font_size as f64 * self.scale as f64).into());

        text_context.add(
            scene,
            None,
            self.font_size * self.scale as f32,
            Some(self.text_color),
            transform,
            contents,
        )
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
                    let shape = self.frame.outer_rect;

                    // Fill the color
                    scene.fill(
                        Fill::NonZero,
                        self.transform,
                        Color::RED,
                        // bg_color.as_vello(),
                        Option::None,
                        &shape,
                    );
                }
                Rect(_) => todo!("Implement background drawing for Image::Rect"),
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
                repeating,
                compat_mode,
                ..
            } => self.draw_linear_gradient(scene, direction, items),
            GenericGradient::Radial {
                shape,
                position,
                items,
                repeating,
                compat_mode,
                ..
            } => self.draw_radial_gradient(scene, shape, position, items, *repeating),
            GenericGradient::Conic {
                angle,
                position,
                items,
                repeating,
                ..
            } => self.draw_conic_gradient(scene, angle, position, items, *repeating),
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
                    let offset = position.to_percentage().unwrap().0;
                    let color = color.as_vello();
                    (color, offset)
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
                        gradient.stops.push(
                            dbg! {peniko::ColorStop { color: mid_color, offset: mid_offset }},
                        );
                        gradient.stops.push(peniko::ColorStop { color, offset });
                    }
                }
            }
        }
        let brush = peniko::BrushRef::Gradient(&gradient);
        scene.fill(peniko::Fill::NonZero, self.transform, brush, None, &shape);
    }

    fn draw_image_frame(&self, scene: &mut Scene) {}

    fn draw_solid_frame(&self, scene: &mut Scene) {
        let background = self.style.get_background();

        // todo: handle non-absolute colors
        let bg_color = background.background_color.clone();
        let bg_color = bg_color.as_absolute().unwrap();
        let shape = self.frame.frame();

        // Fill the color
        scene.fill(
            Fill::NonZero,
            self.transform,
            bg_color.as_vello(),
            None,
            &shape,
        );
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
            BorderStyle::Inset => unimplemented!(),
            BorderStyle::Groove => unimplemented!(),
            BorderStyle::Outset => unimplemented!(),
            BorderStyle::Ridge => unimplemented!(),
            BorderStyle::Dotted => unimplemented!(),
            BorderStyle::Dashed => unimplemented!(),
            BorderStyle::Double => unimplemented!(),
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
    fn stroke_effects(&self, scene: &mut Scene) {
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
        let effects = self.style.get_effects();
    }

    fn stroke_box_shadow(&self, scene: &mut Scene) {
        let effects = self.style.get_effects();
    }

    fn draw_radial_gradient(
        &self,
        scene: &mut Scene,
        shape: &EndingShape<NonNegative<CSSPixelLength>, NonNegative<LengthPercentage>>,
        position: &GenericPosition<LengthPercentage, LengthPercentage>,
        items: &OwnedSlice<GenericGradientItem<StyloColor<Percentage>, LengthPercentage>>,
        repeating: bool,
    ) {
        unimplemented!()
    }

    fn draw_conic_gradient(
        &self,
        scene: &mut Scene,
        angle: &Angle,
        position: &GenericPosition<LengthPercentage, LengthPercentage>,
        items: &OwnedSlice<GenericGradientItem<StyloColor<Percentage>, AngleOrPercentage>>,
        repeating: bool,
    ) {
        unimplemented!()
    }
}
