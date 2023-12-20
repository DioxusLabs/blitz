use dioxus::core::Mutation;
use dioxus::core::Mutations;
use html5ever::tree_builder::QuirksMode::NoQuirks;
// use quadtree_rs::area::AreaBuilder;
// use quadtree_rs::Quadtree;
// use rustc_hash::FxHashSet;
use shipyard::Component;
use shipyard::View;
use std::sync::{Mutex, MutexGuard, RwLock, RwLockWriteGuard};
use style::media_queries::Device as StyleDevice;
use style::media_queries::MediaList;
use style::media_queries::MediaType;
use style::stylesheets::AllowImportRules;
use style::stylesheets::DocumentStyleSheet;
use style::stylesheets::Origin;
use style::stylesheets::Stylesheet;
use style::stylist::Stylist;
use taffy::geometry::Point;
use taffy::prelude::Layout;
use taffy::prelude::Taffy;
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
use crate::text::TextContext;
use crate::viewport::Viewport;
use crate::RealDom;

/// A rendering instance, not necessarily tied to a window
///
pub struct Document {
    dom: RealDom,

    /// The styling engine of firefox
    stylist: Stylist,

    /// The actual viewport of the page that we're getting a glimpse of.
    /// We need this since the part of the page that's being viewed might not be the page in its entirety.
    /// This will let us opt of rendering some stuff
    viewport: Viewport,

    /// Our drawing kit, not necessarily tied to a surface
    renderer: VelloRenderer,

    surface: RenderSurface,

    render_context: RenderContext,

    /// Our text stencil to be used with vello
    text: TextContext,

    /// Our image cache
    images: ImageCache,

    /// A storage of fonts to load in and out.
    /// Whenever we encounter new fonts during parsing + mutations, this will become populated
    fonts: FontCache,
}

impl Document {
    pub async fn from_window(window: &Window) -> Self {
        // 1. Set up renderer-specific stuff
        // We build an independent viewport which can be dynamically set later
        // The intention here is to split the rendering pipeline away from tao/windowing for rendering to images
        let size = window.inner_size();
        let viewport = Viewport::new(size);

        // 2. Set up Vello specific stuff
        let mut render_context;
        let surface;
        let renderer = {
            render_context = RenderContext::new().unwrap();

            surface = render_context
                .create_surface(window, size.width, size.height)
                .await
                .expect("Error creating surface");

            let device = &render_context.devices[surface.dev_id].device;

            let options = RendererOptions {
                surface_format: Some(surface.config.format),
                timestamp_period: render_context.devices[surface.dev_id]
                    .queue
                    .get_timestamp_period(),
            };

            VelloRenderer::new(device, &options).unwrap()
        };

        // 3. Build the dom itself
        let dom = RealDom::new(
            r#"
            <body>
                <h1 class="heading"> h1 </h1>
                <h2 class="heading"> h2 </h2>
                <h3 class="heading"> h3 </h3>
                <h4 class="heading"> h4 </h4>
            </body>
        "#
            .to_string(),
        );

        // 4. Build out stylo, inserting some default stylesheets
        let quirks = selectors::matching::QuirksMode::NoQuirks;
        let stylist = Stylist::new(
            StyleDevice::new(
                MediaType::screen(),
                quirks,
                euclid::Size2D::new(size.width as _, size.height as _),
                euclid::Scale::new(1.0),
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
            text: Default::default(),
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
            selectors::matching::QuirksMode::NoQuirks,
            0,
            AllowImportRules::Yes,
        );

        self.stylist
            .append_stylesheet(DocumentStyleSheet(Arc::new(data)), &self.dom.guard.read())
    }

    // Adjust the viewport
    pub(crate) fn set_size(&mut self, physical_size: PhysicalSize<u32>) {
        self.viewport.window_size = physical_size;
        self.surface.config.height = physical_size.height;
        self.surface.config.width = physical_size.width;
    }

    pub fn handle_mutations(&mut self, mutations: Mutations) {
        use Mutation::*;

        for edit in mutations.edits {
            match edit {
                AppendChildren { id, m } => todo!(),
                AssignId { path, id } => todo!(),
                CreatePlaceholder { id } => todo!(),
                CreateTextNode { value, id } => todo!(),
                HydrateText { path, value, id } => todo!(),
                LoadTemplate { name, index, id } => todo!(),
                ReplaceWith { id, m } => todo!(),
                ReplacePlaceholder { path, m } => todo!(),
                InsertAfter { id, m } => todo!(),
                InsertBefore { id, m } => todo!(),
                SetAttribute {
                    name,
                    value,
                    id,
                    ns,
                } => todo!(),
                SetText { value, id } => todo!(),
                NewEventListener { name, id } => todo!(),
                RemoveEventListener { name, id } => todo!(),
                Remove { id } => todo!(),
                PushRoot { id } => todo!(),
            }
        }
    }

    /// Restyle the tree and then relayout it
    pub fn prepare(&mut self) {
        // taffy::compute_layout(taffy, root, available_space)
    }

    /// Draw the current tree to current render surface
    /// Eventually we'll want the surface itself to be passed into the render function, along with things like the viewport
    ///
    /// This assumes styles are resolved and layout is complete.
    /// Make sure you do those before trying to render
    pub(crate) fn render(&mut self) {
        let mut scene = Scene::new();
        let mut builder = SceneBuilder::for_scene(&mut scene);

        crate::render::render(
            &self.dom,
            &mut self.text,
            &mut builder,
            self.viewport.window_size,
        );

        let surface_texture = self
            .surface
            .surface
            .get_current_texture()
            .expect("failed to get surface texture");

        let device = &self.render_context.devices[self.surface.dev_id];

        self.renderer
            .render_to_surface(
                &device.device,
                &device.queue,
                &scene,
                &surface_texture,
                &RenderParams {
                    base_color: Color::RED,
                    width: self.surface.config.width,
                    height: self.surface.config.height,
                },
            )
            .expect("failed to render to surface");

        surface_texture.present();
        device.device.poll(wgpu::Maintain::Wait);
    }
}
