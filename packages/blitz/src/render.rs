mod multicolor_rounded_rect;

use std::num::NonZeroUsize;
use std::sync::Arc;
// So many imports
use self::multicolor_rounded_rect::{Edge, ElementFrame};
use crate::{
    devtools::Devtools,
    util::{GradientSlice, StyloGradient, ToVelloColor},
    viewport::Viewport,
};
use blitz_dom::node::TextBrush;
use blitz_dom::{
    events::{EventData, RendererEvent},
    node::{NodeData, TextLayout, TextNodeData},
    DocumentLike, Node,
};
use html5ever::local_name;
use image::{imageops::FilterType, DynamicImage};
use parley::layout::LayoutItem2;
use style::{
    dom::TElement,
    values::{
        computed::ui::CursorKind, generics::image::GradientFlags,
        specified::position::HorizontalPositionKeyword,
    },
};
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
use wgpu::{PresentMode, SurfaceError, WasmNotSend};

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

pub struct Renderer<'s, W, Doc: DocumentLike> {
    pub dom: Doc,

    pub render_state: RenderState<'s, W>,

    pub(crate) render_context: RenderContext,

    /// Our image cache
    // pub(crate) images: ImageCache,

    /// A storage of fonts to load in and out.
    /// Whenever we encounter new fonts during parsing + mutations, this will become populated
    // pub(crate) fonts: FontCache,
    pub devtools: Devtools,

    scroll_offset: f64,
    mouse_pos: (f32, f32),
}

impl<'a, W, Doc: DocumentLike> Renderer<'a, W, Doc>
where
    W: raw_window_handle::HasWindowHandle
        + raw_window_handle::HasDisplayHandle
        + Sync
        + WasmNotSend
        + 'a,
{
    pub fn new(dom: Doc) -> Self {
        // 1. Set up renderer-specific stuff
        // We build an independent viewport which can be dynamically set later
        // The intention here is to split the rendering pipeline away from tao/windowing for rendering to images

        // 2. Set up Vello specific stuff
        let render_context = RenderContext::new().unwrap();

        Self {
            render_context,
            render_state: RenderState::Suspended(None),
            dom,
            devtools: Default::default(),
            scroll_offset: 0.0,
            mouse_pos: (0.0, 0.0),
        }
    }

    pub fn poll(&mut self, cx: std::task::Context) -> bool {
        self.dom.poll(cx)
    }

    pub async fn resume(&mut self, window_builder: impl FnOnce() -> (Arc<W>, Viewport)) {
        let RenderState::Suspended(cached_window) = &mut self.render_state else {
            return;
        };

        let (window, viewport) = cached_window.take().unwrap_or_else(window_builder);

        let device = viewport.make_device();
        self.dom.as_mut().set_stylist_device(device);
        self.dom.as_mut().set_scale(viewport.scale());

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

        self.dom.as_mut().resolve();
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
        let RenderState::Active(state) = &self.render_state else {
            return false;
        };

        let x = x / state.viewport.zoom();
        let y = y / state.viewport.zoom();

        // println!("Mouse move: ({}, {})", x, y);
        // println!("Unscaled: ({}, {})",);

        self.dom.as_mut().set_hover_to(x, y)
    }

    pub fn get_cursor(&self) -> Option<CursorKind> {
        // todo: cache this on the node itself
        let node = &self.dom.as_ref().tree()[self.dom.as_ref().get_hover_node_id()?];

        let style = node.primary_styles()?;
        let keyword = style.clone_cursor().keyword;
        let cursor = match keyword {
            CursorKind::Auto => {
                // if the target is text, it's text cursor
                // todo: our "hit" function doesn't return text, only elements
                // this will need to be more comprehensive in the future to handle line breaks, shaping, etc.
                if node.is_text_node() {
                    CursorKind::Text
                } else {
                    CursorKind::Auto
                }
            }
            cusor => cusor,
        };

        Some(cursor)
    }

    pub fn scroll_by(&mut self, px: f64) {
        // Invert scrolling on macos
        #[cfg(target_os = "macos")]
        {
            self.scroll_offset += px;
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.scroll_offset -= px;
        }

        self.clamp_scroll();
    }

    /// Clamp scroll offset
    fn clamp_scroll(&mut self) {
        let content_height = self.dom.as_ref().root_element().final_layout.size.height as f64;
        let viewport_height = self
            .dom
            .as_mut()
            .stylist_device()
            .au_viewport_size()
            .height
            .to_f64_px();
        self.scroll_offset = self
            .scroll_offset
            .min(0.0)
            .max(-(content_height - viewport_height));
    }

    pub fn click(&mut self) {
        let Some(node_id) = self.dom.as_ref().get_hover_node_id() else {
            return;
        };

        let RenderState::Active(_) = &self.render_state else {
            return;
        };

        if self.devtools.highlight_hover {
            let node = &self.dom.as_ref().get_node(node_id).unwrap();
            dbg!(&node.final_layout);
            dbg!(&node.style);

            println!("Node {} {}", node.id, node.node_debug_str());
            if node.is_inline_root {
                let inline_layout = &node
                    .raw_dom_data
                    .downcast_element()
                    .unwrap()
                    .inline_layout
                    .as_ref()
                    .unwrap();

                println!("Text content: {:?}", inline_layout.text);
                println!("Inline Boxes:");
                for ibox in inline_layout.layout.inline_boxes() {
                    print!("(id: {}) ", ibox.id);
                }
                println!();
                println!("Lines:");
                for (i, line) in inline_layout.layout.lines().enumerate() {
                    println!("Line {i}:");
                    for item in line.items() {
                        print!("  ");
                        match item {
                            LayoutItem2::GlyphRun(run) => {
                                print!(
                                    "RUN (x: {}, w: {}) ",
                                    run.offset().round(),
                                    run.run().advance()
                                )
                            }
                            LayoutItem2::InlineBox(ibox) => print!(
                                "BOX (id: {} x: {} y: {} w: {}, h: {})",
                                ibox.id,
                                ibox.x.round(),
                                ibox.y.round(),
                                ibox.width.round(),
                                ibox.height.round()
                            ),
                        }
                        println!();
                    }
                }
            }

            let children: Vec<_> = node
                .children
                .iter()
                .map(|id| &self.dom.as_ref().tree()[*id])
                .map(|node| (node.id, node.order(), node.node_debug_str()))
                .collect();

            println!("Children: {:?}", children);

            let layout_children: Vec<_> = node
                .layout_children
                .borrow()
                .as_ref()
                .unwrap()
                .iter()
                .map(|id| &self.dom.as_ref().tree()[*id])
                .map(|node| (node.id, node.order(), node.node_debug_str()))
                .collect();

            println!("Layout Children: {:?}", layout_children);
            // taffy::print_tree(&self.dom, node_id.into());
        }

        // If we hit a node, then we collect the node to its parents, check for listeners, and then
        // call those listeners
        if !self.devtools.highlight_hover {
            self.dom.handle_event(RendererEvent {
                name: "click".to_string(),
                target: node_id,
                data: EventData::Click {
                    x: self.mouse_pos.0 as f64,
                    y: self.mouse_pos.1 as f64,
                },
            });
        }
    }

    pub fn print_taffy_tree(&self) {
        taffy::print_tree(self.dom.as_ref(), taffy::NodeId::from(0usize));
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
                .as_mut()
                .set_stylist_device(state.viewport.make_device());
            self.dom.as_mut().set_scale(state.viewport.scale());
            self.render_context
                .resize_surface(&mut state.surface, width, height);
            self.clamp_scroll();
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

        let RenderState::Active(state) = &mut self.render_state else {
            return;
        };

        let surface_texture = match state.surface.surface.get_current_texture() {
            Ok(surface) => surface,
            // When resizing too aggresively, the surface can get outdated (another resize) before being rendered into
            Err(SurfaceError::Outdated) => return,
            Err(_) => panic!("failed to get surface texture"),
        };

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
                scene,
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

        let cx = self.element_cx(element, location);
        cx.stroke_effects(scene);
        cx.stroke_outline(scene);
        cx.stroke_frame(scene);
        cx.stroke_border(scene);
        cx.stroke_devtools(scene);
        cx.draw_image(scene);

        if element.is_inline_root {
            let (_layout, pos) = self.node_position(node_id, location);
            let text_layout = &element
                .raw_dom_data
                .downcast_element()
                .unwrap()
                .inline_layout
                .as_ref()
                .unwrap_or_else(|| {
                    dbg!(&element);
                    panic!("Tried to render node marked as inline root that does not have an inline layout");
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
            cx.stroke_text(scene, text_layout, pos);

            // Render inline boxes
            for line in text_layout.layout.lines() {
                for item in line.items() {
                    if let LayoutItem2::InlineBox(ibox) = item {
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
                unreachable!()
            }
            NodeData::Document => {}
            // NodeData::Doctype => {}
            NodeData::Comment => {} // NodeData::ProcessingInstruction { .. } => {}
        }
    }

    fn element_cx<'w>(&'w self, element: &'w Node, location: Point) -> ElementCx {
        let RenderState::Active(state) = &self.render_state else {
            panic!("Renderer is not active");
        };

        let style = element
            .stylo_element_data
            .borrow()
            .as_ref()
            .map(|element_data| element_data.styles.primary().clone())
            .unwrap_or(ComputedValues::initial_values().to_arc());

        let (layout, pos) = self.node_position(element.id, location);
        let scale = state.viewport.scale_f64();

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
            image: element.element_data().unwrap().image.clone(),
            devtools: &self.devtools,
        }
    }

    fn node_position(&self, node: usize, location: Point) -> (Layout, Point) {
        let layout = self.layout(node);
        let pos = location + Vec2::new(layout.location.x as f64, layout.location.y as f64);
        (layout, pos)
    }

    fn layout(&self, child: usize) -> Layout {
        self.dom.as_ref().tree()[child].unrounded_layout
        // self.dom.tree()[child].final_layout
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
    image: Option<Arc<DynamicImage>>,
    devtools: &'a Devtools,
}

impl ElementCx<'_> {
    fn stroke_text(&self, scene: &mut Scene, text_layout: &TextLayout, pos: Point) {
        let transform = Affine::translate((pos.x * self.scale, pos.y * self.scale));

        for line in text_layout.layout.lines() {
            for item in line.items() {
                if let LayoutItem2::GlyphRun(glyph_run) = item {
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

                    scene
                        .draw_glyphs(font)
                        .brush(style.brush.color)
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
                            brush.color,
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

    fn draw_image(&self, scene: &mut Scene) {
        let transform = Affine::translate((self.pos.x * self.scale, self.pos.y * self.scale));

        let width = self.frame.inner_rect.width() as u32;
        let height = self.frame.inner_rect.height() as u32;

        if let Some(image) = &self.image {
            let mut resized_image = self
                .element
                .element_data()
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

    // fn draw_image_frame(&self, scene: &mut Scene) {}

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
