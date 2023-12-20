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
use style::selector_parser::SnapshotMap;
use style::stylesheets::AllowImportRules;
use style::stylesheets::DocumentStyleSheet;
use style::stylesheets::Origin;
use style::stylesheets::Stylesheet;
use style::stylist::Stylist;
use taffy::geometry::Point;
use taffy::prelude::Layout;
use taffy::prelude::Size;
use taffy::prelude::Style;
use taffy::prelude::Taffy;
use taffy::style::AvailableSpace;
use taffy::style::Dimension;
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
use crate::BlitzNode;
use crate::RealDom;

/// A rendering instance, not necessarily tied to a window
///
pub struct Document {
    dom: RealDom,

    layout: Taffy,

    /// The styling engine of firefox
    stylist: Stylist,

    // caching for the stylist
    snapshots: SnapshotMap,

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
    pub async fn from_window(window: &Window, dom: RealDom) -> Self {
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
            snapshots: SnapshotMap::new(),
            layout: Default::default(),
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
            .append_stylesheet(DocumentStyleSheet(Arc::new(data)), &self.dom.guard.read());

        self.stylist
            .force_stylesheet_origins_dirty(Origin::Author.into());
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
    pub fn resolve(&mut self) {
        // we need to resolve stylist first since it will need to drive our layout bits
        self.resolve_stylist();

        // Next we resolve layout with the data resolved by stlist
        self.resolve_layout();
    }

    /// Walk the nodes now that they're properly styled and transfer their styles to the taffy style system
    /// Ideally we could just break apart the styles into ECS bits, but alas
    ///
    /// Todo: update taffy to use an associated type instead of slab key
    /// Todo: update taffy to support traited styles so we don't even need to rely on taffy for storage
    pub fn resolve_layout(&mut self) {
        fn merge_dom(taffy: &mut Taffy, node: BlitzNode) -> taffy::prelude::Node {
            let data = node.data();

            // 1. merge what we can, if we have to
            use markup5ever_rcdom::NodeData;
            let style = match &data.node.data {
                // need to add a measure function?
                NodeData::Text { contents } => {
                    // todo
                    let mut style = Style::DEFAULT;
                    style.size = Size {
                        height: taffy::prelude::Dimension::Points(100.0),
                        width: taffy::prelude::Dimension::Points(100.0),
                    };
                    Some(style)
                    // Some(translate_stylo_to_taffy(node, data))
                }

                // merge element via its attrs
                NodeData::Element { name, attrs, .. } => {
                    // let attrs = attrs.borrow();
                    // for attr in attrs.iter() {}

                    // Get the stylo data for this
                    Some(translate_stylo_to_taffy(node, data))
                }

                NodeData::Document
                | NodeData::Doctype { .. }
                | NodeData::Comment { .. }
                | NodeData::ProcessingInstruction { .. } => None,
            };

            // 2. Insert a leaf into taffy to associate with this node
            let leaf = taffy.new_leaf(style.unwrap_or_default()).unwrap();
            data.layout_id.set(Some(leaf));

            // 3. walk to to children and merge them too
            for idx in data.children.iter() {
                let child = node.with(*idx);
                let child_layout = merge_dom(taffy, child);
                taffy.add_child(leaf, child_layout);
            }

            leaf
        }

        // walk the tree, updating the taffy leaf in place
        // We're gonna construct the tree in place every time so there's a fresh layout
        // This is yucky but fastest way to get the prototype out
        // Assumes the elements are properly styled
        //
        // todo: fix this
        let mut layout = Taffy::new();
        let root = self.dom.root_element();

        let root_key = merge_dom(&mut layout, root);

        let width = self.viewport.window_size.width as f32;
        let height = self.viewport.window_size.height as f32;
        let available_space = Size {
            width: AvailableSpace::Definite(width as _),
            height: AvailableSpace::Definite(height as _),
        };

        let root_layout_id = root.data().layout_id.get().unwrap();

        // root style
        {
            let mut root_style = layout.style(root_layout_id).unwrap().clone();
            root_style.size = Size {
                width: Dimension::Points(width),
                height: Dimension::Points(height),
            };
            layout.set_style(root_layout_id, root_style);
        }

        layout
            .compute_layout(root_layout_id, available_space)
            .unwrap();

        self.layout = layout;
    }

    pub fn resolve_stylist(&mut self) {
        pub use crate::style_impls::{BlitzNode, RealDom};
        use crate::{style_impls, style_traverser};
        use style::{
            animation::DocumentAnimationSet,
            context::{QuirksMode, SharedStyleContext},
            driver,
            global_style_data::GLOBAL_STYLE_DATA,
            media_queries::MediaType,
            media_queries::{Device as StyleDevice, MediaList},
            selector_parser::SnapshotMap,
            servo_arc::Arc,
            shared_lock::{SharedRwLock, StylesheetGuards},
            stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet},
            stylist::Stylist,
            thread_state::ThreadState,
            traversal::DomTraversal,
            traversal_flags::TraversalFlags,
        };

        style::thread_state::enter(ThreadState::LAYOUT);

        let guard = &self.dom.guard;
        let guards = StylesheetGuards {
            author: &guard.read(),
            ua_or_user: &guard.read(),
        };

        // Note that html5ever parses the first node as the document, so we need to unwrap it and get the first child
        // For the sake of this demo, it's always just a single body node, but eventually we will want to construct something like the
        // BoxTree struct that servo uses.
        self.stylist.flush(
            &guards,
            Some(self.dom.root_element()),
            Some(&self.snapshots),
        );

        // Build the style context used by the style traversal
        let context = SharedStyleContext {
            traversal_flags: TraversalFlags::empty(),
            stylist: &self.stylist,
            options: GLOBAL_STYLE_DATA.options.clone(),
            guards,
            visited_styles_enabled: false,
            animations: (&DocumentAnimationSet::default()).clone(),
            current_time_for_animations: 0.0,
            snapshot_map: &self.snapshots,
            registered_speculative_painters: &style_impls::RegisteredPaintersImpl,
        };

        // components/layout_2020/lib.rs:983
        println!("------Pre-traversing the DOM tree -----");
        let root = self.dom.root_element();

        let token = style_traverser::RecalcStyle::pre_traverse(root, &context);

        // Style the elements, resolving their data
        println!("------ Traversing domtree ------",);
        let traverser = style_traverser::RecalcStyle::new(context);
        driver::traverse_dom(&traverser, token, None);

        // now print out the style data
        fn print_styles(markup: &crate::RealDom) {
            use style::dom::{TElement, TNode};

            let root = markup.root_node();
            for node in 0..markup.nodes.len() {
                let Some(el) = root.with(node).as_element() else {
                    continue;
                };

                let data = el.borrow_data().unwrap();
                let primary = data.styles.primary();
                let bg_color = &primary.get_background().background_color;

                println!(
                    "Styles for node {node_idx}:\n{:#?}",
                    bg_color,
                    node_idx = node
                );
            }
        }

        print_styles(&self.dom);
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
            &self.layout,
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

fn translate_stylo_to_taffy(node: BlitzNode, data: &crate::style_impls::NodeData) -> Style {
    let style_data = data.style.borrow();
    let primary = style_data.styles.primary();

    let mut style = Style::DEFAULT;

    let _box = primary.get_box();

    style
}
