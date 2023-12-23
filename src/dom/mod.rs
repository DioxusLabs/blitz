use std::cell::RefCell;

use dioxus::core::{Mutation, Mutations};
use glazier::raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
use selectors::matching::QuirksMode;
use style::{
    media_queries::{Device as StyleDevice, MediaList, MediaType},
    selector_parser::SnapshotMap,
    stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet},
    stylist::Stylist,
};
use taffy::{
    geometry::Point,
    prelude::{Layout, Size, Style, TaffyTree},
    style::{AvailableSpace, Dimension},
};
use tao::{dpi::PhysicalSize, event_loop::EventLoopProxy, window::Window};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use vello::{
    peniko::Color,
    util::{RenderContext, RenderSurface},
    RenderParams, Scene, SceneBuilder,
};
use vello::{Renderer as VelloRenderer, RendererOptions};

use crate::fontcache::FontCache;
use crate::imagecache::ImageCache;
use crate::styling::BlitzNode;
use crate::styling::RealDom;
use crate::text::TextContext;
use crate::viewport::Viewport;

use self::styling::NodeData;

mod layout;
mod mutation;
pub mod styling;

/// A rendering instance, not necessarily tied to a window
///
pub struct Document {
    pub(crate) dom: RealDom,

    pub(crate) taffy: TaffyTree,

    /// The styling engine of firefox
    pub(crate) stylist: Stylist,

    // caching for the stylist
    pub(crate) snapshots: SnapshotMap,

    /// The actual viewport of the page that we're getting a glimpse of.
    /// We need this since the part of the page that's being viewed might not be the page in its entirety.
    /// This will let us opt of rendering some stuff
    pub(crate) viewport: Viewport,

    /// Our drawing kit, not necessarily tied to a surface
    pub(crate) renderer: VelloRenderer,

    pub(crate) surface: RenderSurface,

    pub(crate) render_context: RenderContext,

    /// Our text stencil to be used with vello
    pub(crate) text_context: TextContext,

    /// Our image cache
    pub(crate) images: ImageCache,

    /// A storage of fonts to load in and out.
    /// Whenever we encounter new fonts during parsing + mutations, this will become populated
    pub(crate) fonts: FontCache,
}

impl Document {
    pub async fn from_window<W>(window: W, dom: RealDom, viewport: Viewport) -> Self
    where
        W: HasRawWindowHandle + HasRawDisplayHandle,
    {
        // 1. Set up renderer-specific stuff
        // We build an independent viewport which can be dynamically set later
        // The intention here is to split the rendering pipeline away from tao/windowing for rendering to images

        // 2. Set up Vello specific stuff
        let mut render_context = RenderContext::new().unwrap();
        let surface = render_context
            .create_surface(
                &window,
                viewport.window_size.width,
                viewport.window_size.height,
            )
            .await
            .expect("Error creating surface");

        let device = &render_context.devices[surface.dev_id];
        let options = RendererOptions {
            surface_format: Some(surface.config.format),
            timestamp_period: device.queue.get_timestamp_period(),
        };
        let renderer = VelloRenderer::new(&device.device, &options).unwrap();

        // 4. Build out stylo, inserting some default stylesheets
        let quirks = QuirksMode::NoQuirks;
        let stylist = Stylist::new(
            StyleDevice::new(
                MediaType::screen(),
                quirks,
                euclid::Size2D::new(
                    viewport.window_size.width as _,
                    viewport.window_size.height as _,
                ),
                euclid::Scale::new(viewport.hidpi_scale as _),
            ),
            quirks,
        );

        // 5. Build helpers for things like event handlers, hit testing
        Self {
            viewport,
            render_context,
            renderer,
            surface,
            dom,
            stylist,
            snapshots: SnapshotMap::new(),
            taffy: Default::default(),
            text_context: Default::default(),
            images: Default::default(),
            fonts: Default::default(),
        }
    }

    pub fn add_stylesheet(&mut self, css: &str) {
        use style::servo_arc::Arc;

        let data = Stylesheet::from_str(
            css,
            servo_url::ServoUrl::from_url("data:text/css;charset=utf-8;base64,".parse().unwrap()),
            Origin::UserAgent,
            Arc::new(self.dom.guard.wrap(MediaList::empty())),
            self.dom.guard.clone(),
            None,
            None,
            QuirksMode::NoQuirks,
            0,
            AllowImportRules::Yes,
        );

        self.stylist
            .append_stylesheet(DocumentStyleSheet(Arc::new(data)), &self.dom.guard.read());

        self.stylist
            .force_stylesheet_origins_dirty(Origin::Author.into());
    }

    /// Restyle the tree and then relayout it
    pub fn resolve(&mut self) {
        // we need to resolve stylist first since it will need to drive our layout bits
        self.resolve_stylist();

        // Next we resolve layout with the data resolved by stlist
        self.resolve_layout();
    }

    // Adjust the viewport
    pub(crate) fn set_size(&mut self, size: PhysicalSize<u32>) {
        self.viewport.window_size = size;

        if size.width > 0 && size.height > 0 {
            // self.dom.set_size(size);
            self.render_context
                .resize_surface(&mut self.surface, size.width, size.height);
        }
    }

    /// Draw the current tree to current render surface
    /// Eventually we'll want the surface itself to be passed into the render function, along with things like the viewport
    ///
    /// This assumes styles are resolved and layout is complete.
    /// Make sure you do those before trying to render
    pub(crate) fn render(&mut self, scene: &mut Scene) {
        println!("rendering!");

        self.render_internal(&mut SceneBuilder::for_scene(scene));

        let surface_texture = self
            .surface
            .surface
            .get_current_texture()
            .expect("failed to get surface texture");

        let device = &self.render_context.devices[self.surface.dev_id];

        let render_params = RenderParams {
            base_color: Color::WHITE,
            width: self.surface.config.width,
            height: self.surface.config.height,
        };

        self.renderer
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
}
