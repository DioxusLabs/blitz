use crate::events::handle_dom_event;
use crate::font_metrics::BlitzFontMetricsProvider;
use crate::layout::construct::ConstructionTask;
use crate::layout::damage::ALL_DAMAGE;
use crate::mutator::ViewportMut;
use crate::net::{
    Resource, ResourceHandler, ResourceLoadResponse, StylesheetHandler, StylesheetLoader,
};
use crate::node::{ImageData, NodeFlags, RasterImageData, SpecialElementData, Status, TextBrush};
use crate::selection::TextSelection;
use crate::stylo_to_cursor_icon::stylo_to_cursor_icon;
use crate::traversal::TreeTraverser;
use crate::url::DocumentUrl;
use crate::util::ImageType;
use crate::{
    DEFAULT_CSS, DocumentConfig, DocumentMutator, DummyHtmlParserProvider, ElementData,
    EventDriver, HtmlParserProvider, Node, NodeData, NoopEventHandler, TextNodeData,
};
use blitz_traits::devtools::DevtoolSettings;
use blitz_traits::events::{BlitzScrollEvent, DomEvent, DomEventData, HitResult, UiEvent};
use blitz_traits::navigation::{DummyNavigationProvider, NavigationProvider};
use blitz_traits::net::{DummyNetProvider, NetProvider, Request};
use blitz_traits::shell::{ColorScheme, DummyShellProvider, ShellProvider, Viewport};
use cursor_icon::CursorIcon;
use linebender_resource_handle::Blob;
use markup5ever::local_name;
use parley::{FontContext, PlainEditorDriver};
use selectors::{Element, matching::QuirksMode};
use slab::Slab;
use std::any::Any;
use std::cell::RefCell;
use std::collections::{BTreeMap, Bound, HashMap, HashSet};
use std::ops::{Deref, DerefMut};
use std::rc::Rc;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, RwLockReadGuard, RwLockWriteGuard};
use std::task::Context as TaskContext;
use std::time::Instant;
use style::Atom;
use style::animation::DocumentAnimationSet;
use style::attr::{AttrIdentifier, AttrValue};
use style::data::{ElementData as StyloElementData, ElementStyles};
use style::media_queries::MediaType;
use style::properties::ComputedValues;
use style::properties::style_structs::Font;
use style::queries::values::PrefersColorScheme;
use style::selector_parser::ServoElementSnapshot;
use style::servo_arc::Arc as ServoArc;
use style::values::GenericAtomIdent;
use style::values::computed::Overflow;
use style::{
    dom::{TDocument, TNode},
    media_queries::{Device, MediaList},
    selector_parser::SnapshotMap,
    shared_lock::{SharedRwLock, StylesheetGuards},
    stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet},
    stylist::Stylist,
};
use url::Url;

#[cfg(feature = "parallel-construct")]
use thread_local::ThreadLocal;

pub enum DocGuard<'a> {
    Ref(&'a BaseDocument),
    RefCell(std::cell::Ref<'a, BaseDocument>),
    RwLock(RwLockReadGuard<'a, BaseDocument>),
    Mutex(MutexGuard<'a, BaseDocument>),
}

impl Deref for DocGuard<'_> {
    type Target = BaseDocument;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Ref(base_document) => base_document,
            Self::RefCell(refcell_guard) => refcell_guard,
            Self::RwLock(rw_lock_read_guard) => rw_lock_read_guard,
            Self::Mutex(mutex_guard) => mutex_guard,
        }
    }
}

pub enum DocGuardMut<'a> {
    Ref(&'a mut BaseDocument),
    RefCell(std::cell::RefMut<'a, BaseDocument>),
    RwLock(RwLockWriteGuard<'a, BaseDocument>),
    Mutex(MutexGuard<'a, BaseDocument>),
}

impl Deref for DocGuardMut<'_> {
    type Target = BaseDocument;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Ref(base_document) => base_document,
            Self::RefCell(refcell_guard) => refcell_guard,
            Self::RwLock(rw_lock_read_guard) => rw_lock_read_guard,
            Self::Mutex(mutex_guard) => mutex_guard,
        }
    }
}

impl DerefMut for DocGuardMut<'_> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Ref(base_document) => base_document,
            Self::RefCell(refcell_guard) => &mut *refcell_guard,
            Self::RwLock(rw_lock_read_guard) => &mut *rw_lock_read_guard,
            Self::Mutex(mutex_guard) => &mut *mutex_guard,
        }
    }
}

/// Abstraction over wrappers around [`BaseDocument`] to allow for them all to
/// be driven by [`blitz-shell`](https://docs.rs/blitz-shell)
pub trait Document: Any + 'static {
    fn inner(&self) -> DocGuard<'_>;
    fn inner_mut(&mut self) -> DocGuardMut<'_>;

    /// Update the [`Document`] in response to a [`UiEvent`] (click, keypress, etc)
    fn handle_ui_event(&mut self, event: UiEvent) {
        let mut doc = self.inner_mut();
        let mut driver = EventDriver::new(&mut *doc, NoopEventHandler);
        driver.handle_ui_event(event);
    }

    /// Poll any pending async operations, and flush changes to the underlying [`BaseDocument`]
    fn poll(&mut self, task_context: Option<TaskContext>) -> bool {
        // Default implementation does nothing
        let _ = task_context;
        false
    }

    /// Get the [`Document`]'s id
    fn id(&self) -> usize {
        self.inner().id
    }
}

pub struct PlainDocument(pub BaseDocument);
impl Document for PlainDocument {
    fn inner(&self) -> DocGuard<'_> {
        DocGuard::Ref(&self.0)
    }
    fn inner_mut(&mut self) -> DocGuardMut<'_> {
        DocGuardMut::Ref(&mut self.0)
    }
}

impl Document for BaseDocument {
    fn inner(&self) -> DocGuard<'_> {
        DocGuard::Ref(self)
    }
    fn inner_mut(&mut self) -> DocGuardMut<'_> {
        DocGuardMut::Ref(self)
    }
}

impl Document for Rc<RefCell<BaseDocument>> {
    fn inner(&self) -> DocGuard<'_> {
        DocGuard::RefCell(self.borrow())
    }

    fn inner_mut(&mut self) -> DocGuardMut<'_> {
        DocGuardMut::RefCell(self.borrow_mut())
    }
}

pub enum DocumentEvent {
    ResourceLoad(ResourceLoadResponse),
}

pub struct BaseDocument {
    /// ID of the document
    id: usize,

    // Config
    /// Base url for resolving linked resources (stylesheets, images, fonts, etc)
    pub(crate) url: DocumentUrl,
    // Devtool settings. Currently used to render debug overlays
    pub(crate) devtool_settings: DevtoolSettings,
    // Viewport details such as the dimensions, HiDPI scale, and zoom factor,
    pub(crate) viewport: Viewport,
    // Scroll within our viewport
    pub(crate) viewport_scroll: crate::Point<f64>,

    // Events
    pub(crate) tx: Sender<DocumentEvent>,
    // rx will always be Some, except temporarily while processing events
    pub(crate) rx: Option<Receiver<DocumentEvent>>,

    /// A slab-backed tree of nodes
    ///
    /// We pin the tree to a guarantee to the nodes it creates that the tree is stable in memory.
    /// There is no way to create the tree - publicly or privately - that would invalidate that invariant.
    pub(crate) nodes: Box<Slab<Node>>,

    // Stylo
    /// The Stylo engine
    pub(crate) stylist: Stylist,
    pub(crate) animations: DocumentAnimationSet,
    /// Stylo shared lock
    pub(crate) guard: SharedRwLock,
    /// Stylo invalidation map. We insert into this map prior to mutating nodes.
    pub(crate) snapshots: SnapshotMap,

    // Parley contexts
    /// A Parley font context
    pub(crate) font_ctx: Arc<Mutex<parley::FontContext>>,
    #[cfg(feature = "parallel-construct")]
    /// Thread-and-document-local copies to the font context
    pub(crate) thread_font_contexts: ThreadLocal<RefCell<Box<FontContext>>>,
    /// A Parley layout context
    pub(crate) layout_ctx: parley::LayoutContext<TextBrush>,

    /// The node which is currently hovered (if any)
    pub(crate) hover_node_id: Option<usize>,
    /// Whether the node which is currently hovered is a text node/span
    pub(crate) hover_node_is_text: bool,
    /// The node which is currently focussed (if any)
    pub(crate) focus_node_id: Option<usize>,
    /// The node which is currently active (if any)
    pub(crate) active_node_id: Option<usize>,
    /// The node which recieved a mousedown event (if any)
    pub(crate) mousedown_node_id: Option<usize>,
    /// The last time a mousedown was made (for double-click detection)
    pub(crate) last_mousedown_time: Option<Instant>,
    /// The position where mousedown occurred (for selection drags and double-click detection)
    pub(crate) mousedown_position: taffy::Point<f32>,
    /// How many clicks have been made in quick succession
    pub(crate) click_count: u16,
    /// Whether we're currently in a text selection drag (moved 2px+ from mousedown)
    pub(crate) is_selecting: bool,

    /// Text selection state (for non-input text)
    pub(crate) text_selection: TextSelection,

    // TODO: collapse animating state into a bitflags
    /// Whether there are active CSS animations/transitions (so we should re-render every frame)
    pub(crate) has_active_animations: bool,
    /// Whether there is a <canvas> element in the DOM (so we should re-render every frame)
    pub(crate) has_canvas: bool,
    /// Whether there are subdocuments that are animating (so we should re-render every frame)
    pub(crate) subdoc_is_animating: bool,

    /// Map of node ID's for fast lookups
    pub(crate) nodes_to_id: HashMap<String, usize>,
    /// Map of `<style>` and `<link>` node IDs to their associated stylesheet
    pub(crate) nodes_to_stylesheet: BTreeMap<usize, DocumentStyleSheet>,
    /// Stylesheets added by the useragent
    /// where the key is the hashed CSS
    pub(crate) ua_stylesheets: HashMap<String, DocumentStyleSheet>,
    /// Map from form control node ID's to their associated forms node ID's
    pub(crate) controls_to_form: HashMap<usize, usize>,
    /// Nodes that contain sub documents
    pub(crate) sub_document_nodes: HashSet<usize>,
    /// Set of changed nodes for updating the accessibility tree
    pub(crate) changed_nodes: HashSet<usize>,
    /// Set of changed nodes for updating the accessibility tree
    pub(crate) deferred_construction_nodes: Vec<ConstructionTask>,

    /// Cache of loaded images, keyed by URL. Allows reusing images across multiple
    /// elements without re-fetching from the network.
    pub(crate) image_cache: HashMap<String, ImageData>,

    /// Tracks in-flight image requests. When an image is being fetched, additional
    /// requests for the same URL are queued here instead of starting new fetches.
    /// Value is a list of (node_id, image_type) pairs waiting for the image.
    pub(crate) pending_images: HashMap<String, Vec<(usize, ImageType)>>,

    // Service providers
    /// Network provider. Can be used to fetch assets.
    pub net_provider: Arc<dyn NetProvider>,
    /// Navigation provider. Can be used to navigate to a new page (bubbles up the event
    /// on e.g. clicking a Link)
    pub navigation_provider: Arc<dyn NavigationProvider>,
    /// Shell provider. Can be used to request a redraw or set the cursor icon
    pub shell_provider: Arc<dyn ShellProvider>,
    /// HTML parser provider. Used to parse HTML for setInnerHTML
    pub html_parser_provider: Arc<dyn HtmlParserProvider>,
}

pub(crate) fn make_device(viewport: &Viewport, font_ctx: Arc<Mutex<FontContext>>) -> Device {
    let width = viewport.window_size.0 as f32 / viewport.scale();
    let height = viewport.window_size.1 as f32 / viewport.scale();
    let viewport_size = euclid::Size2D::new(width, height);
    let device_pixel_ratio = euclid::Scale::new(viewport.scale());

    Device::new(
        MediaType::screen(),
        selectors::matching::QuirksMode::NoQuirks,
        viewport_size,
        device_pixel_ratio,
        Box::new(BlitzFontMetricsProvider { font_ctx }),
        ComputedValues::initial_values_with_font_override(Font::initial_values()),
        match viewport.color_scheme {
            ColorScheme::Light => PrefersColorScheme::Light,
            ColorScheme::Dark => PrefersColorScheme::Dark,
        },
    )
}

impl BaseDocument {
    /// Create a new (empty) [`BaseDocument`] with the specified configuration
    pub fn new(config: DocumentConfig) -> Self {
        static ID_GENERATOR: AtomicUsize = AtomicUsize::new(1);

        let id = ID_GENERATOR.fetch_add(1, Ordering::SeqCst);

        let font_ctx = config
            .font_ctx
            // .map(|mut font_ctx| {
            //     font_ctx.collection.make_shared();
            //     font_ctx.source_cache.make_shared();
            //     font_ctx
            // })
            .unwrap_or_else(|| {
                // let mut font_ctx = FontContext {
                //     source_cache: SourceCache::new_shared(),
                //     collection: Collection::new(CollectionOptions {
                //         shared: true,
                //         system_fonts: true,
                //     }),
                // };
                let mut font_ctx = FontContext::default();
                font_ctx
                    .collection
                    .register_fonts(Blob::new(Arc::new(crate::BULLET_FONT) as _), None);
                font_ctx
            });
        let font_ctx = Arc::new(Mutex::new(font_ctx));

        let viewport = config.viewport.unwrap_or_default();
        let device = make_device(&viewport, font_ctx.clone());
        let stylist = Stylist::new(device, QuirksMode::NoQuirks);
        let snapshots = SnapshotMap::new();
        let nodes = Box::new(Slab::new());
        let guard = SharedRwLock::new();
        let nodes_to_id = HashMap::new();

        // Make sure we turn on stylo features
        style_config::set_bool("layout.flexbox.enabled", true);
        style_config::set_bool("layout.grid.enabled", true);
        style_config::set_bool("layout.legacy_layout", true);
        style_config::set_bool("layout.unimplemented", true);
        style_config::set_bool("layout.columns.enabled", true);

        let base_url = config
            .base_url
            .and_then(|url| DocumentUrl::from_str(&url).ok())
            .unwrap_or_default();

        let net_provider = config
            .net_provider
            .unwrap_or_else(|| Arc::new(DummyNetProvider));
        let navigation_provider = config
            .navigation_provider
            .unwrap_or_else(|| Arc::new(DummyNavigationProvider));
        let shell_provider = config
            .shell_provider
            .unwrap_or_else(|| Arc::new(DummyShellProvider));
        let html_parser_provider = config
            .html_parser_provider
            .unwrap_or_else(|| Arc::new(DummyHtmlParserProvider));

        let (tx, rx) = channel();

        let mut doc = Self {
            id,
            tx,
            rx: Some(rx),

            guard,
            nodes,
            stylist,
            animations: DocumentAnimationSet::default(),
            snapshots,
            nodes_to_id,
            viewport,
            devtool_settings: DevtoolSettings::default(),
            viewport_scroll: crate::Point::ZERO,
            url: base_url,
            ua_stylesheets: HashMap::new(),
            nodes_to_stylesheet: BTreeMap::new(),
            font_ctx,
            #[cfg(feature = "parallel-construct")]
            thread_font_contexts: ThreadLocal::new(),
            layout_ctx: parley::LayoutContext::new(),

            hover_node_id: None,
            hover_node_is_text: false,
            focus_node_id: None,
            active_node_id: None,
            mousedown_node_id: None,
            has_active_animations: false,
            subdoc_is_animating: false,
            has_canvas: false,
            sub_document_nodes: HashSet::new(),
            changed_nodes: HashSet::new(),
            deferred_construction_nodes: Vec::new(),
            image_cache: HashMap::new(),
            pending_images: HashMap::new(),
            controls_to_form: HashMap::new(),
            net_provider,
            navigation_provider,
            shell_provider,
            html_parser_provider,
            last_mousedown_time: None,
            mousedown_position: taffy::Point::ZERO,
            click_count: 0,
            is_selecting: false,
            text_selection: TextSelection::default(),
        };

        // Initialise document with root Document node
        doc.create_node(NodeData::Document);
        doc.root_node_mut().flags.insert(NodeFlags::IS_IN_DOCUMENT);

        match config.ua_stylesheets {
            Some(stylesheets) => {
                for ss in &stylesheets {
                    doc.add_user_agent_stylesheet(ss);
                }
            }
            None => doc.add_user_agent_stylesheet(DEFAULT_CSS),
        }

        // Stylo data on the root node container is needed to render the node
        let stylo_element_data = StyloElementData {
            styles: ElementStyles {
                primary: Some(
                    ComputedValues::initial_values_with_font_override(Font::initial_values())
                        .to_arc(),
                ),
                ..Default::default()
            },
            ..Default::default()
        };
        *doc.root_node().stylo_element_data.borrow_mut() = Some(stylo_element_data);

        doc
    }

    /// Set the Document's networking provider
    pub fn set_net_provider(&mut self, net_provider: Arc<dyn NetProvider>) {
        self.net_provider = net_provider;
    }

    /// Set the Document's navigation provider
    pub fn set_navigation_provider(&mut self, navigation_provider: Arc<dyn NavigationProvider>) {
        self.navigation_provider = navigation_provider;
    }

    /// Set the Document's shell provider
    pub fn set_shell_provider(&mut self, shell_provider: Arc<dyn ShellProvider>) {
        self.shell_provider = shell_provider;
    }

    /// Set the Document's html parser provider
    pub fn set_html_parser_provider(&mut self, html_parser_provider: Arc<dyn HtmlParserProvider>) {
        self.html_parser_provider = html_parser_provider;
    }

    /// Set base url for resolving linked resources (stylesheets, images, fonts, etc)
    pub fn set_base_url(&mut self, url: &str) {
        self.url = DocumentUrl::from(Url::parse(url).unwrap());
    }

    pub fn guard(&self) -> &SharedRwLock {
        &self.guard
    }

    pub fn tree(&self) -> &Slab<Node> {
        &self.nodes
    }

    pub fn id(&self) -> usize {
        self.id
    }

    pub fn get_node(&self, node_id: usize) -> Option<&Node> {
        self.nodes.get(node_id)
    }

    pub fn get_node_mut(&mut self, node_id: usize) -> Option<&mut Node> {
        self.nodes.get_mut(node_id)
    }

    pub fn get_focussed_node_id(&self) -> Option<usize> {
        self.focus_node_id
            .or(self.try_root_element().map(|el| el.id))
    }

    pub fn mutate<'doc>(&'doc mut self) -> DocumentMutator<'doc> {
        DocumentMutator::new(self)
    }

    pub fn handle_dom_event<F: FnMut(DomEvent)>(
        &mut self,
        event: &mut DomEvent,
        dispatch_event: F,
    ) {
        handle_dom_event(self, event, dispatch_event)
    }

    pub fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    /// Find the label's bound input elements:
    /// the element id referenced by the "for" attribute of a given label element
    /// or the first input element which is nested in the label
    /// Note that although there should only be one bound element,
    /// we return all possibilities instead of just the first
    /// in order to allow the caller to decide which one is correct
    pub fn label_bound_input_element(&self, label_node_id: usize) -> Option<&Node> {
        let label_element = self.nodes[label_node_id].element_data()?;
        if let Some(target_element_dom_id) = label_element.attr(local_name!("for")) {
            TreeTraverser::new(self)
                .filter_map(|id| {
                    let node = self.get_node(id)?;
                    let element_data = node.element_data()?;
                    if element_data.name.local != local_name!("input") {
                        return None;
                    }
                    let id = element_data.id.as_ref()?;
                    if *id == *target_element_dom_id {
                        Some(node)
                    } else {
                        None
                    }
                })
                .next()
        } else {
            TreeTraverser::new_with_root(self, label_node_id)
                .filter_map(|child_id| {
                    let node = self.get_node(child_id)?;
                    let element_data = node.element_data()?;
                    if element_data.name.local == local_name!("input") {
                        Some(node)
                    } else {
                        None
                    }
                })
                .next()
        }
    }

    pub fn toggle_checkbox(el: &mut ElementData) -> bool {
        let Some(is_checked) = el.checkbox_input_checked_mut() else {
            return false;
        };
        *is_checked = !*is_checked;

        *is_checked
    }

    pub fn toggle_radio(&mut self, radio_set_name: String, target_radio_id: usize) {
        for i in 0..self.nodes.len() {
            let node = &mut self.nodes[i];
            if let Some(node_data) = node.data.downcast_element_mut() {
                if node_data.attr(local_name!("name")) == Some(&radio_set_name) {
                    let was_clicked = i == target_radio_id;
                    let Some(is_checked) = node_data.checkbox_input_checked_mut() else {
                        continue;
                    };
                    *is_checked = was_clicked;
                }
            }
        }
    }

    pub fn set_style_property(&mut self, node_id: usize, name: &str, value: &str) {
        self.nodes[node_id]
            .element_data_mut()
            .unwrap()
            .set_style_property(name, value, &self.guard, self.url.url_extra_data());
    }

    pub fn remove_style_property(&mut self, node_id: usize, name: &str) {
        self.nodes[node_id]
            .element_data_mut()
            .unwrap()
            .remove_style_property(name, &self.guard, self.url.url_extra_data());
    }

    pub fn set_sub_document(&mut self, node_id: usize, sub_document: Box<dyn Document>) {
        self.nodes[node_id]
            .element_data_mut()
            .unwrap()
            .set_sub_document(sub_document);
        self.sub_document_nodes.insert(node_id);
    }

    pub fn remove_sub_document(&mut self, node_id: usize) {
        self.nodes[node_id]
            .element_data_mut()
            .unwrap()
            .remove_sub_document();
        self.sub_document_nodes.remove(&node_id);
    }

    pub fn root_node(&self) -> &Node {
        &self.nodes[0]
    }

    pub fn root_node_mut(&mut self) -> &mut Node {
        &mut self.nodes[0]
    }

    pub fn try_root_element(&self) -> Option<&Node> {
        TDocument::as_node(&self.root_node()).first_element_child()
    }

    pub fn root_element(&self) -> &Node {
        TDocument::as_node(&self.root_node())
            .first_element_child()
            .unwrap()
            .as_element()
            .unwrap()
    }

    pub fn create_node(&mut self, node_data: NodeData) -> usize {
        let slab_ptr = self.nodes.as_mut() as *mut Slab<Node>;
        let guard = self.guard.clone();

        let entry = self.nodes.vacant_entry();
        let id = entry.key();
        entry.insert(Node::new(slab_ptr, id, guard, node_data));

        // Mark the new node as changed.
        self.changed_nodes.insert(id);
        id
    }

    pub(crate) fn drop_node_ignoring_parent(&mut self, node_id: usize) -> Option<Node> {
        let mut node = self.nodes.try_remove(node_id);
        if let Some(node) = &mut node {
            if let Some(before) = node.before {
                self.drop_node_ignoring_parent(before);
            }
            if let Some(after) = node.after {
                self.drop_node_ignoring_parent(after);
            }

            for &child in &node.children {
                self.drop_node_ignoring_parent(child);
            }
        }
        node
    }

    /// Whether the document has been mutated
    pub fn has_changes(&self) -> bool {
        self.changed_nodes.is_empty()
    }

    pub fn create_text_node(&mut self, text: &str) -> usize {
        let content = text.to_string();
        let data = NodeData::Text(TextNodeData::new(content));
        self.create_node(data)
    }

    pub fn deep_clone_node(&mut self, node_id: usize) -> usize {
        // Load existing node
        let node = &self.nodes[node_id];
        let data = node.data.clone();
        let children = node.children.clone();

        // Create new node
        let new_node_id = self.create_node(data);

        // Recursively clone children
        let new_children: Vec<usize> = children
            .into_iter()
            .map(|child_id| self.deep_clone_node(child_id))
            .collect();
        for &child_id in &new_children {
            self.nodes[child_id].parent = Some(new_node_id);
        }
        self.nodes[new_node_id].children = new_children;

        new_node_id
    }

    pub(crate) fn remove_and_drop_pe(&mut self, node_id: usize) -> Option<Node> {
        fn remove_pe_ignoring_parent(doc: &mut BaseDocument, node_id: usize) -> Option<Node> {
            let mut node = doc.nodes.try_remove(node_id);
            if let Some(node) = &mut node {
                for &child in &node.children {
                    remove_pe_ignoring_parent(doc, child);
                }
            }
            node
        }

        let node = remove_pe_ignoring_parent(self, node_id);

        // Update child_idx values
        if let Some(parent_id) = node.as_ref().and_then(|node| node.parent) {
            let parent = &mut self.nodes[parent_id];
            parent.children.retain(|id| *id != node_id);
        }

        node
    }

    pub(crate) fn resolve_url(&self, raw: &str) -> url::Url {
        self.url.resolve_relative(raw).unwrap_or_else(|| {
            panic!(
                "to be able to resolve {raw} with the base_url: {:?}",
                *self.url
            )
        })
    }

    pub fn print_tree(&self) {
        crate::util::walk_tree(0, self.root_node());
    }

    pub fn print_subtree(&self, node_id: usize) {
        crate::util::walk_tree(0, &self.nodes[node_id]);
    }

    pub fn reload_resource_by_href(&mut self, href_to_reload: &str) {
        for &node_id in self.nodes_to_stylesheet.keys() {
            let node = &self.nodes[node_id];
            let Some(element) = node.element_data() else {
                continue;
            };

            if element.name.local == local_name!("link") {
                if let Some(href) = element.attr(local_name!("href")) {
                    // println!("Node {node_id} {href} {href_to_reload} {} {}", resolved_href.as_str(), resolved_href.as_str() == url_to_reload);
                    if href == href_to_reload {
                        let resolved_href = self.resolve_url(href);
                        self.net_provider.fetch(
                            self.id(),
                            Request::get(resolved_href.clone()),
                            ResourceHandler::boxed(
                                self.tx.clone(),
                                self.id,
                                Some(node_id),
                                self.shell_provider.clone(),
                                StylesheetHandler {
                                    source_url: resolved_href,
                                    guard: self.guard.clone(),
                                    net_provider: self.net_provider.clone(),
                                },
                            ),
                        );
                    }
                }
            }
        }
    }

    pub fn process_style_element(&mut self, target_id: usize) {
        let css = self.nodes[target_id].text_content();
        let css = html_escape::decode_html_entities(&css);
        let sheet = self.make_stylesheet(&css, Origin::Author);
        self.add_stylesheet_for_node(sheet, target_id);
    }

    pub fn remove_user_agent_stylesheet(&mut self, contents: &str) {
        if let Some(sheet) = self.ua_stylesheets.remove(contents) {
            self.stylist.remove_stylesheet(sheet, &self.guard.read());
        }
    }

    pub fn add_user_agent_stylesheet(&mut self, css: &str) {
        let sheet = self.make_stylesheet(css, Origin::UserAgent);
        self.ua_stylesheets.insert(css.to_string(), sheet.clone());
        self.stylist.append_stylesheet(sheet, &self.guard.read());
    }

    pub fn make_stylesheet(&self, css: impl AsRef<str>, origin: Origin) -> DocumentStyleSheet {
        let data = Stylesheet::from_str(
            css.as_ref(),
            self.url.url_extra_data(),
            origin,
            ServoArc::new(self.guard.wrap(MediaList::empty())),
            self.guard.clone(),
            Some(&StylesheetLoader {
                tx: self.tx.clone(),
                doc_id: self.id,
                net_provider: self.net_provider.clone(),
                shell_provider: self.shell_provider.clone(),
            }),
            None,
            QuirksMode::NoQuirks,
            AllowImportRules::Yes,
        );

        DocumentStyleSheet(ServoArc::new(data))
    }

    pub fn upsert_stylesheet_for_node(&mut self, node_id: usize) {
        let raw_styles = self.nodes[node_id].text_content();
        let sheet = self.make_stylesheet(raw_styles, Origin::Author);
        self.add_stylesheet_for_node(sheet, node_id);
    }

    pub fn add_stylesheet_for_node(&mut self, stylesheet: DocumentStyleSheet, node_id: usize) {
        let old = self.nodes_to_stylesheet.insert(node_id, stylesheet.clone());

        if let Some(old) = old {
            self.stylist.remove_stylesheet(old, &self.guard.read())
        }

        // Fetch @font-face fonts
        crate::net::fetch_font_face(
            self.tx.clone(),
            self.id,
            Some(node_id),
            &stylesheet.0,
            &self.net_provider,
            &self.shell_provider,
            &self.guard.read(),
        );

        // Store data on element
        let element = &mut self.nodes[node_id].element_data_mut().unwrap();
        element.special_data = SpecialElementData::Stylesheet(stylesheet.clone());

        // TODO: Nodes could potentially get reused so ordering by node_id might be wrong.
        let insertion_point = self
            .nodes_to_stylesheet
            .range((Bound::Excluded(node_id), Bound::Unbounded))
            .next()
            .map(|(_, sheet)| sheet);

        if let Some(insertion_point) = insertion_point {
            self.stylist.insert_stylesheet_before(
                stylesheet,
                insertion_point.clone(),
                &self.guard.read(),
            )
        } else {
            self.stylist
                .append_stylesheet(stylesheet, &self.guard.read())
        }
    }

    pub fn handle_messages(&mut self) {
        // Remove event Reciever from the Document so that we can process events
        // without holding a borrow to the Document
        let rx = self.rx.take().unwrap();

        while let Ok(msg) = rx.try_recv() {
            self.handle_message(msg);
        }

        // Put Reciever back
        self.rx = Some(rx);
    }

    pub fn handle_message(&mut self, msg: DocumentEvent) {
        match msg {
            DocumentEvent::ResourceLoad(resource) => self.load_resource(resource),
        }
    }

    pub fn load_resource(&mut self, res: ResourceLoadResponse) {
        let Ok(resource) = res.result else {
            // TODO: handle error
            return;
        };

        match resource {
            Resource::Css(css) => {
                let node_id = res.node_id.unwrap();
                self.add_stylesheet_for_node(css, node_id);
            }
            Resource::Image(_kind, width, height, image_data) => {
                // Create the ImageData and cache it
                let image = ImageData::Raster(RasterImageData::new(width, height, image_data));

                let Some(url) = res.resolved_url.as_ref() else {
                    return;
                };

                // Get all nodes waiting for this image
                let waiting_nodes = self.pending_images.remove(url).unwrap_or_default();

                #[cfg(feature = "tracing")]
                tracing::info!(
                    "Image {url} loaded, applying to {} nodes",
                    waiting_nodes.len()
                );

                // Cache the image
                self.image_cache.insert(url.clone(), image.clone());

                // Apply to all waiting nodes
                for (node_id, image_type) in waiting_nodes {
                    let Some(node) = self.get_node_mut(node_id) else {
                        continue;
                    };

                    match image_type {
                        ImageType::Image => {
                            node.element_data_mut().unwrap().special_data =
                                SpecialElementData::Image(Box::new(image.clone()));

                            // Clear layout cache
                            node.cache.clear();
                            node.insert_damage(ALL_DAMAGE);
                        }
                        ImageType::Background(idx) => {
                            if let Some(Some(bg_image)) = node
                                .element_data_mut()
                                .and_then(|el| el.background_images.get_mut(idx))
                            {
                                bg_image.status = Status::Ok;
                                bg_image.image = image.clone();
                            }
                        }
                    }
                }
            }
            #[cfg(feature = "svg")]
            Resource::Svg(_kind, tree) => {
                // Create the ImageData and cache it
                let image = ImageData::Svg(tree);

                let Some(url) = res.resolved_url.as_ref() else {
                    return;
                };

                // Get all nodes waiting for this image
                let waiting_nodes = self.pending_images.remove(url).unwrap_or_default();

                #[cfg(feature = "tracing")]
                tracing::info!(
                    "SVG {url} loaded, applying to {} nodes",
                    waiting_nodes.len()
                );

                // Cache the image
                self.image_cache.insert(url.clone(), image.clone());

                // Apply to all waiting nodes
                for (node_id, image_type) in waiting_nodes {
                    let Some(node) = self.get_node_mut(node_id) else {
                        continue;
                    };

                    match image_type {
                        ImageType::Image => {
                            node.element_data_mut().unwrap().special_data =
                                SpecialElementData::Image(Box::new(image.clone()));

                            // Clear layout cache
                            node.cache.clear();
                            node.insert_damage(ALL_DAMAGE);
                        }
                        ImageType::Background(idx) => {
                            if let Some(Some(bg_image)) = node
                                .element_data_mut()
                                .and_then(|el| el.background_images.get_mut(idx))
                            {
                                bg_image.status = Status::Ok;
                                bg_image.image = image.clone();
                            }
                        }
                    }
                }
            }
            Resource::Font(bytes) => {
                let font = Blob::new(Arc::new(bytes));

                // TODO: Implement FontInfoOveride
                // TODO: Investigate eliminating double-box
                let mut global_font_ctx = self.font_ctx.lock().unwrap();
                global_font_ctx
                    .collection
                    .register_fonts(font.clone(), None);

                #[cfg(feature = "parallel-construct")]
                {
                    rayon::broadcast(|_ctx| {
                        let mut font_ctx = self
                            .thread_font_contexts
                            .get_or(|| RefCell::new(Box::new(global_font_ctx.clone())))
                            .borrow_mut();
                        font_ctx.collection.register_fonts(font.clone(), None);
                    });
                }
                drop(global_font_ctx);

                // TODO: see if we can only invalidate if resolved fonts may have changed
                self.invalidate_inline_contexts();
            }
            Resource::None => {
                // Do nothing
            }
        }
    }

    pub fn snapshot_node(&mut self, node_id: usize) {
        let node = &mut self.nodes[node_id];
        let opaque_node_id = TNode::opaque(&&*node);
        node.has_snapshot = true;
        node.snapshot_handled
            .store(false, std::sync::atomic::Ordering::SeqCst);

        // TODO: handle invalidations other than hover
        if let Some(_existing_snapshot) = self.snapshots.get_mut(&opaque_node_id) {
            // Do nothing
            // TODO: update snapshot
        } else {
            let attrs: Option<Vec<_>> = node.attrs().map(|attrs| {
                attrs
                    .iter()
                    .map(|attr| {
                        let ident = AttrIdentifier {
                            local_name: GenericAtomIdent(attr.name.local.clone()),
                            name: GenericAtomIdent(attr.name.local.clone()),
                            namespace: GenericAtomIdent(attr.name.ns.clone()),
                            prefix: None,
                        };

                        let value = if attr.name.local == local_name!("id") {
                            AttrValue::Atom(Atom::from(&*attr.value))
                        } else if attr.name.local == local_name!("class") {
                            let classes = attr
                                .value
                                .split_ascii_whitespace()
                                .map(Atom::from)
                                .collect();
                            AttrValue::TokenList(attr.value.clone(), classes)
                        } else {
                            AttrValue::String(attr.value.clone())
                        };

                        (ident, value)
                    })
                    .collect()
            });

            let changed_attrs = attrs
                .as_ref()
                .map(|attrs| attrs.iter().map(|attr| attr.0.name.clone()).collect())
                .unwrap_or_default();

            self.snapshots.insert(
                opaque_node_id,
                ServoElementSnapshot {
                    state: Some(node.element_state),
                    attrs,
                    changed_attrs,
                    class_changed: true,
                    id_changed: true,
                    other_attributes_changed: true,
                },
            );
        }
    }

    pub fn snapshot_node_and(&mut self, node_id: usize, cb: impl FnOnce(&mut Node)) {
        self.snapshot_node(node_id);
        cb(&mut self.nodes[node_id]);
    }

    // Takes (x, y) co-ordinates (relative to the )
    pub fn hit(&self, x: f32, y: f32) -> Option<HitResult> {
        if TDocument::as_node(&&self.nodes[0])
            .first_element_child()
            .is_none()
        {
            println!("No DOM - not resolving");
            return None;
        }

        self.root_element().hit(x, y)
    }

    pub fn focus_next_node(&mut self) -> Option<usize> {
        let focussed_node_id = self.get_focussed_node_id()?;
        let id = self.next_node(&self.nodes[focussed_node_id], |node| node.is_focussable())?;
        self.set_focus_to(id);
        Some(id)
    }

    /// Clear the focussed node
    pub fn clear_focus(&mut self) {
        if let Some(id) = self.focus_node_id {
            let shell_provider = self.shell_provider.clone();
            self.snapshot_node_and(id, |node| node.blur(shell_provider));
            self.focus_node_id = None;
        }
    }

    pub fn set_mousedown_node_id(&mut self, node_id: Option<usize>) {
        self.mousedown_node_id = node_id;
    }
    pub fn set_focus_to(&mut self, focus_node_id: usize) -> bool {
        if Some(focus_node_id) == self.focus_node_id {
            return false;
        }

        #[cfg(feature = "tracing")]
        tracing::info!("Focussed node {focus_node_id}");

        let shell_provider = self.shell_provider.clone();

        // Remove focus from the old node
        if let Some(id) = self.focus_node_id {
            self.snapshot_node_and(id, |node| node.blur(shell_provider.clone()));
        }

        // Focus the new node
        self.snapshot_node_and(focus_node_id, |node| node.focus(shell_provider));

        self.focus_node_id = Some(focus_node_id);

        true
    }

    pub fn active_node(&mut self) -> bool {
        let Some(hover_node_id) = self.get_hover_node_id() else {
            return false;
        };

        if let Some(active_node_id) = self.active_node_id {
            if active_node_id == hover_node_id {
                return true;
            }
            self.unactive_node();
        }

        let active_node_id = Some(hover_node_id);

        let node_path = self.maybe_node_layout_ancestors(active_node_id);
        for &id in node_path.iter() {
            self.snapshot_node_and(id, |node| node.active());
        }

        self.active_node_id = active_node_id;

        true
    }

    pub fn unactive_node(&mut self) -> bool {
        let Some(active_node_id) = self.active_node_id.take() else {
            return false;
        };

        let node_path = self.maybe_node_layout_ancestors(Some(active_node_id));
        for &id in node_path.iter() {
            self.snapshot_node_and(id, |node| node.unactive());
        }

        true
    }

    pub fn set_hover_to(&mut self, x: f32, y: f32) -> bool {
        let hit = self.hit(x, y);
        let hover_node_id = hit.map(|hit| hit.node_id);
        let new_is_text = hit.map(|hit| hit.is_text).unwrap_or(false);

        // Return early if the new node is the same as the already-hovered node
        if hover_node_id == self.hover_node_id {
            return false;
        }

        let old_node_path = self.maybe_node_layout_ancestors(self.hover_node_id);
        let new_node_path = self.maybe_node_layout_ancestors(hover_node_id);
        let same_count = old_node_path
            .iter()
            .zip(&new_node_path)
            .take_while(|(o, n)| o == n)
            .count();
        for &id in old_node_path.iter().skip(same_count) {
            self.snapshot_node_and(id, |node| node.unhover());
        }
        for &id in new_node_path.iter().skip(same_count) {
            self.snapshot_node_and(id, |node| node.hover());
        }

        self.hover_node_id = hover_node_id;
        self.hover_node_is_text = new_is_text;

        // Update the cursor
        let cursor = self.get_cursor().unwrap_or_default();
        self.shell_provider.set_cursor(cursor);

        // Request redraw
        self.shell_provider.request_redraw();

        true
    }

    pub fn get_hover_node_id(&self) -> Option<usize> {
        self.hover_node_id
    }

    pub fn set_viewport(&mut self, viewport: Viewport) {
        let scale_has_changed = viewport.scale_f64() != self.viewport.scale_f64();
        self.viewport = viewport;
        self.set_stylist_device(make_device(&self.viewport, self.font_ctx.clone()));
        self.scroll_viewport_by(0.0, 0.0); // Clamp scroll offset

        if scale_has_changed {
            self.invalidate_inline_contexts();
        }
    }

    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    pub fn viewport_mut(&mut self) -> ViewportMut<'_> {
        ViewportMut::new(self)
    }

    pub fn zoom_by(&mut self, increment: f32) {
        *self.viewport.zoom_mut() += increment;
        self.set_viewport(self.viewport.clone());
    }

    pub fn zoom_to(&mut self, zoom: f32) {
        *self.viewport.zoom_mut() = zoom;
        self.set_viewport(self.viewport.clone());
    }

    pub fn get_viewport(&self) -> Viewport {
        self.viewport.clone()
    }

    pub fn devtools(&self) -> &DevtoolSettings {
        &self.devtool_settings
    }

    pub fn devtools_mut(&mut self) -> &mut DevtoolSettings {
        &mut self.devtool_settings
    }

    pub fn is_animating(&self) -> bool {
        self.has_canvas | self.has_active_animations | self.subdoc_is_animating
    }

    /// Update the device and reset the stylist to process the new size
    pub fn set_stylist_device(&mut self, device: Device) {
        let origins = {
            let guard = &self.guard;
            let guards = StylesheetGuards {
                author: &guard.read(),
                ua_or_user: &guard.read(),
            };
            self.stylist.set_device(device, &guards)
        };
        self.stylist.force_stylesheet_origins_dirty(origins);
    }

    pub fn stylist_device(&mut self) -> &Device {
        self.stylist.device()
    }

    pub fn get_cursor(&self) -> Option<CursorIcon> {
        let node = &self.nodes[self.get_hover_node_id()?];

        if let Some(subdoc) = node.subdoc().map(|doc| doc.inner()) {
            return subdoc.get_cursor();
        }

        let style = node.primary_styles()?;
        let keyword = stylo_to_cursor_icon(style.clone_cursor().keyword);

        // Return cursor from style if it is non-auto
        if keyword != CursorIcon::Default {
            return Some(keyword);
        }

        // Return text cursor for text inputs
        if node
            .element_data()
            .is_some_and(|e| e.text_input_data().is_some())
        {
            return Some(CursorIcon::Text);
        }

        // Use "pointer" cursor if any ancestor is a link
        let mut maybe_node = Some(node);
        while let Some(node) = maybe_node {
            if node.is_link() {
                return Some(CursorIcon::Pointer);
            }

            maybe_node = node.layout_parent.get().map(|node_id| node.with(node_id));
        }

        // Return text cursor for text nodes
        if self.hover_node_is_text {
            return Some(CursorIcon::Text);
        }

        // Else fallback to default cursor
        Some(CursorIcon::Default)
    }

    pub fn scroll_node_by<F: FnMut(DomEvent)>(
        &mut self,
        node_id: usize,
        x: f64,
        y: f64,
        dispatch_event: F,
    ) {
        self.scroll_node_by_has_changed(node_id, x, y, dispatch_event);
    }

    /// Scroll a node by given x and y
    /// Will bubble scrolling up to parent node once it can no longer scroll further
    /// If we're already at the root node, bubbles scrolling up to the viewport
    pub fn scroll_node_by_has_changed<F: FnMut(DomEvent)>(
        &mut self,
        node_id: usize,
        x: f64,
        y: f64,
        mut dispatch_event: F,
    ) -> bool {
        let Some(node) = self.nodes.get_mut(node_id) else {
            return false;
        };

        let is_html_or_body = node.data.downcast_element().is_some_and(|e| {
            let tag = &e.name.local;
            tag == "html" || tag == "body"
        });

        let (can_x_scroll, can_y_scroll) = node
            .primary_styles()
            .map(|styles| {
                (
                    matches!(styles.clone_overflow_x(), Overflow::Scroll | Overflow::Auto),
                    matches!(styles.clone_overflow_y(), Overflow::Scroll | Overflow::Auto)
                        || (styles.clone_overflow_y() == Overflow::Visible && is_html_or_body),
                )
            })
            .unwrap_or((false, false));

        let initial = node.scroll_offset;
        let new_x = node.scroll_offset.x - x;
        let new_y = node.scroll_offset.y - y;

        let mut bubble_x = 0.0;
        let mut bubble_y = 0.0;

        let scroll_width = node.final_layout.scroll_width() as f64;
        let scroll_height = node.final_layout.scroll_height() as f64;

        // Handle sub document case
        if let Some(mut sub_doc) = node.subdoc_mut().map(|doc| doc.inner_mut()) {
            let has_changed = if let Some(hover_node_id) = sub_doc.get_hover_node_id() {
                sub_doc.scroll_node_by_has_changed(hover_node_id, x, y, dispatch_event)
            } else {
                sub_doc.scroll_viewport_by_has_changed(x, y)
            };

            // TODO: propagate remaining scroll to parent
            return has_changed;
        }

        // If we're past our scroll bounds, transfer remainder of scrolling to parent/viewport
        if !can_x_scroll {
            bubble_x = x
        } else if new_x < 0.0 {
            bubble_x = -new_x;
            node.scroll_offset.x = 0.0;
        } else if new_x > scroll_width {
            bubble_x = scroll_width - new_x;
            node.scroll_offset.x = scroll_width;
        } else {
            node.scroll_offset.x = new_x;
        }

        if !can_y_scroll {
            bubble_y = y
        } else if new_y < 0.0 {
            bubble_y = -new_y;
            node.scroll_offset.y = 0.0;
        } else if new_y > scroll_height {
            bubble_y = scroll_height - new_y;
            node.scroll_offset.y = scroll_height;
        } else {
            node.scroll_offset.y = new_y;
        }

        let has_changed = node.scroll_offset != initial;

        if has_changed {
            let layout = node.final_layout;
            let event = BlitzScrollEvent {
                scroll_top: node.scroll_offset.y,
                scroll_left: node.scroll_offset.x,
                scroll_width: layout.scroll_width() as i32,
                scroll_height: layout.scroll_height() as i32,
                client_width: layout.size.width as i32,
                client_height: layout.size.height as i32,
            };

            dispatch_event(DomEvent::new(node_id, DomEventData::Scroll(event)));
        }

        if bubble_x != 0.0 || bubble_y != 0.0 {
            if let Some(parent) = node.parent {
                return self.scroll_node_by_has_changed(parent, bubble_x, bubble_y, dispatch_event)
                    | has_changed;
            } else {
                return self.scroll_viewport_by_has_changed(bubble_x, bubble_y) | has_changed;
            }
        }

        has_changed
    }

    pub fn scroll_viewport_by(&mut self, x: f64, y: f64) {
        self.scroll_viewport_by_has_changed(x, y);
    }

    /// Scroll the viewport by the given values
    pub fn scroll_viewport_by_has_changed(&mut self, x: f64, y: f64) -> bool {
        let content_size = self.root_element().final_layout.size;
        let new_scroll = (self.viewport_scroll.x - x, self.viewport_scroll.y - y);
        let window_width = self.viewport.window_size.0 as f64 / self.viewport.scale() as f64;
        let window_height = self.viewport.window_size.1 as f64 / self.viewport.scale() as f64;

        let initial = self.viewport_scroll;
        self.viewport_scroll.x = f64::max(
            0.0,
            f64::min(new_scroll.0, content_size.width as f64 - window_width),
        );
        self.viewport_scroll.y = f64::max(
            0.0,
            f64::min(new_scroll.1, content_size.height as f64 - window_height),
        );

        self.viewport_scroll != initial
    }

    pub fn viewport_scroll(&self) -> crate::Point<f64> {
        self.viewport_scroll
    }

    pub fn set_viewport_scroll(&mut self, scroll: crate::Point<f64>) {
        self.viewport_scroll = scroll;
    }

    /// Computes the size and position of the `Node` relative to the viewport
    pub fn get_client_bounding_rect(&self, node_id: usize) -> Option<BoundingRect> {
        let node = self.get_node(node_id)?;

        let mut pos = node.absolute_position(0.0, 0.0).map(|v| v as f64);
        pos.x -= self.viewport_scroll.x;
        pos.y -= self.viewport_scroll.y;

        Some(BoundingRect {
            x: pos.x,
            y: pos.y,
            width: node.unrounded_layout.size.width as f64,
            height: node.unrounded_layout.size.width as f64,
        })
    }

    pub fn find_title_node(&self) -> Option<&Node> {
        TreeTraverser::new(self)
            .find(|node_id| {
                self.nodes[*node_id]
                    .data
                    .is_element_with_tag_name(&local_name!("title"))
            })
            .map(|node_id| &self.nodes[node_id])
    }

    pub fn with_text_input(
        &mut self,
        node_id: usize,
        cb: impl FnOnce(PlainEditorDriver<TextBrush>),
    ) {
        let Some(node) = self.nodes.get_mut(node_id) else {
            return;
        };

        if let Some(text_input) = node
            .element_data_mut()
            .and_then(|el| el.text_input_data_mut())
        {
            let mut font_ctx = self.font_ctx.lock().unwrap();
            let layout_ctx = &mut self.layout_ctx;
            let driver = text_input.editor.driver(&mut font_ctx, layout_ctx);
            cb(driver)
        }
    }

    pub(crate) fn compute_has_canvas(&self) -> bool {
        TreeTraverser::new(self).any(|node_id| {
            let node = &self.nodes[node_id];
            let Some(element) = node.element_data() else {
                return false;
            };
            if element.name.local == local_name!("canvas") && element.has_attr(local_name!("src")) {
                return true;
            }

            false
        })
    }

    // Text selection methods

    /// Find the text position (inline_root_id, byte_offset) at a given point.
    /// Uses hit() for proper coordinate transformation, then finds the inline root
    /// and byte offset.
    pub fn find_text_position(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        let hit = self.hit(x, y)?;
        let hit_node = self.get_node(hit.node_id)?;
        let inline_root = hit_node.inline_root_ancestor()?;
        let byte_offset = inline_root.text_offset_at_point(hit.x, hit.y)?;
        Some((inline_root.id, byte_offset))
    }

    /// Set the text selection range (creates a new selection from anchor to focus)
    pub fn set_text_selection(
        &mut self,
        anchor_node: usize,
        anchor_offset: usize,
        focus_node: usize,
        focus_offset: usize,
    ) {
        self.text_selection =
            TextSelection::new(anchor_node, anchor_offset, focus_node, focus_offset);

        // For anonymous blocks, switch to storing parent+sibling_index (stable reference)
        if let (Some(parent), Some(idx)) = self.anonymous_block_location(anchor_node) {
            self.text_selection
                .anchor
                .set_anonymous(parent, idx, anchor_offset);
        }
        if let (Some(parent), Some(idx)) = self.anonymous_block_location(focus_node) {
            self.text_selection
                .focus
                .set_anonymous(parent, idx, focus_offset);
        }
    }

    /// Get the parent ID and sibling index for a node if it's an anonymous block.
    /// Returns (None, None) for non-anonymous blocks.
    fn anonymous_block_location(&self, node_id: usize) -> (Option<usize>, Option<usize>) {
        let Some(node) = self.get_node(node_id) else {
            return (None, None);
        };

        if !node.is_anonymous() {
            return (None, None);
        }

        let Some(parent_id) = node.parent else {
            return (None, None);
        };

        let Some(parent) = self.get_node(parent_id) else {
            return (Some(parent_id), None);
        };

        let layout_children = parent.layout_children.borrow();
        let Some(children) = layout_children.as_ref() else {
            return (Some(parent_id), None);
        };

        // Find the index of this anonymous block among siblings
        let mut anon_index = 0;
        for &child_id in children.iter() {
            if child_id == node_id {
                return (Some(parent_id), Some(anon_index));
            }
            if self.get_node(child_id).is_some_and(|n| n.is_anonymous()) {
                anon_index += 1;
            }
        }

        (Some(parent_id), None)
    }

    /// Clear the text selection
    pub fn clear_text_selection(&mut self) {
        self.text_selection.clear();
    }

    /// Update the selection focus point (used during mouse drag to extend selection).
    pub fn update_selection_focus(&mut self, focus_node: usize, focus_offset: usize) {
        // For anonymous blocks, store parent+sibling_index; otherwise store node directly
        if let (Some(parent), Some(idx)) = self.anonymous_block_location(focus_node) {
            self.text_selection
                .focus
                .set_anonymous(parent, idx, focus_offset);
        } else {
            self.text_selection.set_focus(focus_node, focus_offset);
        }
    }

    /// Extend text selection to the given point. Returns true if selection was updated.
    /// This is a convenience method that combines find_text_position and update_selection_focus.
    pub fn extend_text_selection_to_point(&mut self, x: f32, y: f32) -> bool {
        if !self.text_selection.anchor.is_some() {
            return false;
        }

        if let Some((node, offset)) = self.find_text_position(x, y) {
            self.update_selection_focus(node, offset);
            self.shell_provider.request_redraw();
            true
        } else {
            false
        }
    }

    /// Find the Nth anonymous block under a parent.
    fn find_anonymous_block_by_index(
        &self,
        parent_id: usize,
        target_index: usize,
    ) -> Option<usize> {
        let parent = self.get_node(parent_id)?;
        let layout_children = parent.layout_children.borrow();
        let children = layout_children.as_ref()?;

        children
            .iter()
            .filter(|&&child_id| self.get_node(child_id).is_some_and(|n| n.is_anonymous()))
            .nth(target_index)
            .copied()
    }

    /// Check if there is an active (non-empty) text selection
    pub fn has_text_selection(&self) -> bool {
        self.text_selection.is_active()
    }

    /// Get the selected text content, supporting selection across multiple inline roots.
    pub fn get_selected_text(&self) -> Option<String> {
        let ranges = self.get_text_selection_ranges();
        if ranges.is_empty() {
            return None;
        }

        let mut result = String::new();
        for (node_id, start, end) in &ranges {
            let node = self.get_node(*node_id)?;
            let element_data = node.element_data()?;
            let inline_layout = element_data.inline_layout_data.as_ref()?;

            if *end > inline_layout.text.len() {
                continue;
            }

            if !result.is_empty() {
                result.push(' ');
            }
            result.push_str(&inline_layout.text[*start..*end]);
        }

        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// Get all selection ranges as Vec<(node_id, start_offset, end_offset)>.
    /// Returns empty vec if no selection.
    pub fn get_text_selection_ranges(&self) -> Vec<(usize, usize, usize)> {
        let lookup = |parent_id, idx| self.find_anonymous_block_by_index(parent_id, idx);

        let anchor_node = match self.text_selection.anchor.resolve_node_id(lookup) {
            Some(id) => id,
            None => return Vec::new(),
        };
        let focus_node = match self.text_selection.focus.resolve_node_id(lookup) {
            Some(id) => id,
            None => return Vec::new(),
        };

        // Single node selection
        if anchor_node == focus_node {
            let start = self
                .text_selection
                .anchor
                .offset
                .min(self.text_selection.focus.offset);
            let end = self
                .text_selection
                .anchor
                .offset
                .max(self.text_selection.focus.offset);

            if start == end {
                return Vec::new();
            }
            return vec![(anchor_node, start, end)];
        }

        // Multi-node selection: collect all inline roots between anchor and focus
        let inline_roots = self.collect_inline_roots_in_range(anchor_node, focus_node);
        if inline_roots.is_empty() {
            return Vec::new();
        }

        // Determine document order using the collected inline_roots order
        // (inline_roots is already in document order from first to last)
        let first_in_roots = inline_roots[0];

        let (first_node, first_offset, last_node, last_offset) =
            if first_in_roots == anchor_node || (first_in_roots != focus_node) {
                // anchor is first (or neither endpoint is in roots, which shouldn't happen)
                (
                    anchor_node,
                    self.text_selection.anchor.offset,
                    focus_node,
                    self.text_selection.focus.offset,
                )
            } else {
                // focus is first
                (
                    focus_node,
                    self.text_selection.focus.offset,
                    anchor_node,
                    self.text_selection.anchor.offset,
                )
            };

        let mut ranges = Vec::with_capacity(inline_roots.len());

        for &node_id in &inline_roots {
            let Some(node) = self.get_node(node_id) else {
                continue;
            };
            let Some(element_data) = node.element_data() else {
                continue;
            };
            let Some(inline_layout) = element_data.inline_layout_data.as_ref() else {
                continue;
            };

            let text_len = inline_layout.text.len();

            if node_id == first_node && node_id == last_node {
                let start = first_offset.min(last_offset);
                let end = first_offset.max(last_offset);
                if start < end && end <= text_len {
                    ranges.push((node_id, start, end));
                }
            } else if node_id == first_node {
                if first_offset < text_len {
                    ranges.push((node_id, first_offset, text_len));
                }
            } else if node_id == last_node {
                if last_offset > 0 && last_offset <= text_len {
                    ranges.push((node_id, 0, last_offset));
                }
            } else if text_len > 0 {
                ranges.push((node_id, 0, text_len));
            }
        }

        ranges
    }
}

pub struct BoundingRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl AsRef<BaseDocument> for BaseDocument {
    fn as_ref(&self) -> &BaseDocument {
        self
    }
}

impl AsMut<BaseDocument> for BaseDocument {
    fn as_mut(&mut self) -> &mut BaseDocument {
        self
    }
}
