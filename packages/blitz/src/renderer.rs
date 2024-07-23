mod multicolor_rounded_rect;
mod render;

use crate::{devtools::Devtools, viewport::Viewport};
use blitz_dom::{
    events::{EventData, RendererEvent},
    DocumentLike,
};
use parley::layout::PositionedLayoutItem;
use std::num::NonZeroUsize;
use std::sync::Arc;
use style::values::computed::ui::CursorKind;
use vello::{
    peniko::Color, util::RenderContext, util::RenderSurface, AaSupport, RenderParams,
    Renderer as VelloRenderer, RendererOptions, Scene,
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

        const DEFAULT_THREADS: Option<NonZeroUsize> = {
            #[cfg(target_os = "macos")]
            {
                NonZeroUsize::new(1)
            }
            #[cfg(not(target_os = "macos"))]
            {
                None
            }
        };

        let options = RendererOptions {
            surface_format: Some(surface.config.format),
            antialiasing_support: AaSupport::all(),
            use_cpu: false,
            num_init_threads: DEFAULT_THREADS,
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
        let y = (y - self.scroll_offset as f32) / state.viewport.zoom();

        // println!("Mouse move: ({}, {})", x, y);
        // println!("Unscaled: ({}, {})",);

        self.dom.as_mut().set_hover_to(x, y)
    }

    pub fn focus_next_node(&mut self) -> bool {
        self.dom.as_mut().focus_next_node();
        true
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
            .max(-(content_height - viewport_height))
            .min(0.0);
    }

    pub fn click(&mut self, button: &str) {
        let Some(node_id) = self.dom.as_ref().get_hover_node_id() else {
            return;
        };

        let RenderState::Active(_) = &self.render_state else {
            return;
        };

        if self.devtools.highlight_hover {
            let mut node = self.dom.as_ref().get_node(node_id).unwrap();

            if button == "right" {
                if let Some(parent_id) = node.parent {
                    node = self.dom.as_ref().get_node(parent_id).unwrap();
                }
            }

            #[cfg(feature = "tracing")]
            {
                tracing::info!("Layout: {:?}", &node.final_layout);
                tracing::info!("Style: {:?}", &node.style);
            }

            println!("Node {} {}", node.id, node.node_debug_str());
            if node.is_inline_root {
                let inline_layout = &node
                    .raw_dom_data
                    .downcast_element()
                    .unwrap()
                    .inline_layout_data()
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
                            PositionedLayoutItem::GlyphRun(run) => {
                                print!(
                                    "RUN (x: {}, w: {}) ",
                                    run.offset().round(),
                                    run.run().advance()
                                )
                            }
                            PositionedLayoutItem::InlineBox(ibox) => print!(
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
        if !self.devtools.highlight_hover && button == "left" {
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

    pub fn render(&mut self, scene: &mut Scene) {
        self.generate_vello_scene(scene);

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
}
