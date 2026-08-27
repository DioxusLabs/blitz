use crate::NodeTree;
use crate::events::{DragMode, handle_dom_event};
use crate::layout::construct::ConstructionTask;
use crate::layout::damage::ALL_DAMAGE;
use crate::mutator::ViewportMut;
use crate::net::{
    Resource, ResourceHandler, ResourceLoadResponse, StylesheetHandler, StylesheetLoader,
};
use crate::node::{ImageData, NodeFlags, RasterImageData, SpecialElementData, Status, TextBrush};
use crate::scrolling::ScrollAnimationState;
use crate::selection::TextSelection;
use crate::stylo_device::{DeviceChanges, make_device};
use crate::stylo_to_cursor_icon::stylo_to_cursor_icon;
use crate::traversal::TreeTraverser;
use crate::url::DocumentUrl;
use crate::util::ImageType;
use crate::{
    DEFAULT_CSS, DocumentConfig, DocumentMutator, DummyHtmlParserProvider, ElementData,
    EventDriver, HtmlParserProvider, Node, NodeData, NoopEventHandler, StyleThreading,
    TextNodeData,
};
use blitz_traits::devtools::DevtoolSettings;
use blitz_traits::events::{DomEvent, HitResult, UiEvent};
use blitz_traits::navigation::{DummyNavigationProvider, NavigationProvider};
use blitz_traits::net::{AbortSignal, DummyNetProvider, NetProvider, Request};
use blitz_traits::node_id::NodeId;
use blitz_traits::shell::{DummyShellProvider, ShellProvider, Viewport};
use cursor_icon::CursorIcon;
use linebender_resource_handle::Blob;
use markup5ever::{LocalName, local_name};
use parley::{FontContext, PlainEditorDriver};
use selectors::{Element, matching::QuirksMode};
use smallvec::SmallVec;
use std::any::Any;
use std::cell::RefCell;
use std::collections::{BTreeMap, Bound, HashMap, HashSet};
use std::ops::{Deref, DerefMut};
use std::rc::Rc;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLockReadGuard, RwLockWriteGuard};
use std::task::{Context as TaskContext, Waker};
use style::Atom;
use style::animation::DocumentAnimationSet;
use style::attr::{AttrIdentifier, AttrValue};
use style::computed_value_flags::ComputedValueFlags;
use style::data::{ElementData as StyloElementData, ElementStyles};
use style::invalidation::element::restyle_hints::RestyleHint;
use style::media_queries::MediaType;
use style::properties::ComputedValues;
use style::properties::style_structs::Font;
use style::selector_parser::ServoElementSnapshot;
use style::servo_arc::Arc as ServoArc;
use style::values::GenericAtomIdent;
use style::values::computed::UserSelect;
use style::values::computed::ui::CursorKind;
use style::values::specified::box_::{DisplayInside, DisplayOutside};
use style::{
    device::Device,
    dom::{TDocument, TNode},
    media_queries::MediaList,
    selector_parser::SnapshotMap,
    shared_lock::{SharedRwLock, StylesheetGuards},
    stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet},
    stylist::Stylist,
};
use style_dom::ElementState;
use thin_vec::ThinVec;
use url::Url;
use web_time::Instant;

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
    /// A navigation originating from within an iframe's sub-document
    /// (e.g. a link click), to be applied to the iframe identified by `node_id`.
    NavigateIframe {
        node_id: NodeId,
        url: Url,
    },
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
    /// CSS media type used to evaluate `@media` rules.
    pub(crate) media_type: MediaType,
    /// Changes to the stylist [`Device`] that have been requested since the
    /// last [`resolve`](Self::resolve), to be applied (coalesced into a single
    /// device rebuild) at the start of the next resolve.
    pub(crate) pending_device_changes: DeviceChanges,
    /// Strategy for Stylo's style traversal during `resolve`.
    pub(crate) style_threading: StyleThreading,
    /// Whether incremental layout is enabled for this document.
    pub(crate) incremental_layout: bool,
    /// How deeply this document is nested within other documents
    /// (0 for a root document). Used to limit `<iframe>` nesting depth.
    pub(crate) subdocument_depth: usize,

    // Events
    pub(crate) tx: Sender<DocumentEvent>,
    // rx will always be Some, except temporarily while processing events
    pub(crate) rx: Option<Receiver<DocumentEvent>>,

    /// A slotmap-backed tree of nodes
    ///
    /// We pin the tree to a guarantee to the nodes it creates that the tree is stable in memory.
    /// There is no way to create the tree - publicly or privately - that would invalidate that invariant.
    pub(crate) nodes: Box<NodeTree>,

    /// The id of the root node (a Document node)
    pub(crate) root_node_id: NodeId,

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

    /// The real (non-anonymous) node which is currently hovered (if any).
    /// This is never a layout-generated (anonymous) node, so it remains valid
    /// across box-tree reconstruction.
    pub(crate) hover_node_id: Option<NodeId>,
    /// The precise (may be anonymous) layout node under the pointer (if any).
    /// This can be invalidated by box-tree reconstruction, and is re-resolved against
    /// fresh layout at the end of every `resolve` pass.
    pub(crate) hover_hit_node_id: Option<NodeId>,
    /// Whether the node which is currently hovered is a text node/span
    pub(crate) hover_node_is_text: bool,
    /// The last known pointer position in client coordinates (viewport-relative, unscrolled).
    pub(crate) last_client_pointer_position: Option<taffy::Point<f32>>,
    /// The node which is currently focussed (if any)
    pub(crate) focus_node_id: Option<NodeId>,
    /// The node which is currently active (if any)
    pub(crate) active_node_id: Option<NodeId>,
    /// The node which recieved a mousedown event (if any)
    pub(crate) mousedown_node_id: Option<NodeId>,
    /// The last time a mousedown was made (for double-click detection)
    pub(crate) last_mousedown_time: Option<Instant>,
    /// The position where mousedown occurred (for selection drags and double-click detection)
    pub(crate) mousedown_position: taffy::Point<f32>,
    /// How many clicks have been made in quick succession
    pub(crate) click_count: u16,
    /// Whether we're currently in a text selection drag (moved 2px+ from mousedown)
    pub(crate) drag_mode: DragMode,
    /// The scrollbar thumb currently under the pointer, if any
    pub(crate) hovered_scrollbar: Option<crate::node::ScrollbarRef>,
    /// When each scroll container's overlay scrollbars were last shown
    /// (scrolled, or the pointer left the thumb); drives their fade-out
    pub(crate) scrollbar_activity: HashMap<NodeId, Instant>,
    /// Whether and what kind of scroll animation is currently in progress
    pub(crate) scroll_animation: ScrollAnimationState,

    /// Text selection state (for non-input text)
    pub(crate) text_selection: TextSelection,

    // TODO: collapse animating state into a bitflags
    /// Whether there are active CSS animations/transitions (so we should re-render every frame)
    pub(crate) has_active_animations: bool,
    /// Whether there is a `<canvas>` element in the DOM (so we should re-render every frame)
    pub(crate) has_canvas: bool,
    /// Whether there are subdocuments that are animating (so we should re-render every frame)
    pub(crate) subdoc_is_animating: bool,

    /// Map of id attribute values to node IDs for fast lookups.
    /// May contain multiple nodes for the same id: `get_element_by_id`
    /// returns the first in tree order.
    pub(crate) nodes_to_id: HashMap<String, SmallVec<[NodeId; 1]>>,
    /// Map of `<style>` and `<link>` node IDs to their associated stylesheet
    pub(crate) nodes_to_stylesheet: BTreeMap<NodeId, DocumentStyleSheet>,
    /// Stylesheets added by the useragent
    /// where the key is the hashed CSS
    pub(crate) ua_stylesheets: HashMap<String, DocumentStyleSheet>,
    /// Map from form control node ID's to their associated forms node ID's
    pub(crate) controls_to_form: HashMap<NodeId, NodeId>,
    /// Nodes that contain sub documents
    pub(crate) sub_document_nodes: HashSet<NodeId>,
    /// Load state (abort controller and in-flight request id) for each
    /// `<iframe>` element whose sub-document is loaded automatically
    pub(crate) iframe_loads: HashMap<NodeId, crate::iframe::IframeLoad>,
    /// Set of changed nodes for updating the accessibility tree
    pub(crate) changed_nodes: HashSet<NodeId>,
    /// Set of changed nodes for updating the accessibility tree
    pub(crate) deferred_construction_nodes: Vec<ConstructionTask>,

    /// Nodes that contain custom widgets
    #[cfg(feature = "custom-widget")]
    pub(crate) custom_widget_nodes: HashSet<NodeId>,
    /// Rendering resources allocated by custom widgets that should be deallocated during the next render
    #[cfg(feature = "custom-widget")]
    pub(crate) pending_resource_deallocations: Vec<anyrender::ResourceId>,

    /// Cache of loaded images, keyed by URL. Allows reusing images across multiple
    /// elements without re-fetching from the network.
    pub(crate) image_cache: HashMap<String, ImageData>,

    /// Tracks in-flight image requests. When an image is being fetched, additional
    /// requests for the same URL are queued here instead of starting new fetches.
    /// Value is a list of (node_id, image_type) pairs waiting for the image.
    pub(crate) pending_images: HashMap<String, Vec<(NodeId, ImageType)>>,

    /// Nodes whose `background-image`/`mask-image` layers need flushing to
    /// dedicated storage on the node because their style changed (populated by
    /// the style traversal and by pseudo-element box construction).
    pub(crate) pending_style_image_nodes: Vec<NodeId>,

    // Tracks in-flight "critical" resources (e.g. stylesheets linked from the `<head>`),
    // keyed by request id
    pub(crate) pending_critical_resources: HashSet<usize>,

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
    /// Carried on every sub-resource `Request` this document issues; aborting
    /// it cancels all in-flight fetches tied to this document. Set via
    /// [`DocumentConfig::abort_signal`].
    pub(crate) abort_signal: Option<AbortSignal>,
}

impl BaseDocument {
    /// Create a new (empty) [`BaseDocument`] with the specified configuration
    pub fn new(config: DocumentConfig) -> Self {
        static ID_GENERATOR: AtomicUsize = AtomicUsize::new(1);

        let id = ID_GENERATOR.fetch_add(1, Ordering::SeqCst);

        let font_ctx = config
            .font_ctx
            .map(|mut font_ctx| {
                font_ctx.source_cache.make_shared();
                // font_ctx.collection.make_shared();
                font_ctx
            })
            .unwrap_or_else(|| {
                use parley::fontique::{Collection, CollectionOptions, SourceCache};
                let mut font_ctx = FontContext {
                    source_cache: SourceCache::new_shared(),
                    collection: Collection::new(CollectionOptions {
                        shared: false,
                        system_fonts: cfg!(all(
                            feature = "system-fonts",
                            not(target_arch = "wasm32")
                        )),
                    }),
                };
                font_ctx
                    .collection
                    .register_fonts(Blob::new(Arc::new(crate::BULLET_FONT) as _), None);
                font_ctx
            });
        let font_ctx = Arc::new(Mutex::new(font_ctx));

        // Make sure we turn on stylo features *before* creating the Stylist
        style_config::set_pref!("layout.grid.enabled", true);
        style_config::set_pref!("layout.unimplemented", true);
        style_config::set_pref!("layout.columns.enabled", true);
        style_config::set_pref!("layout.css.basic-shape-shape.enabled", true);
        style_config::set_pref!("layout.threads", -1);

        let viewport = config.viewport.unwrap_or_default();
        let media_type = config.media_type.unwrap_or_else(MediaType::screen);
        let device = make_device(&viewport, media_type.clone(), font_ctx.clone());
        let stylist = Stylist::new(device, QuirksMode::NoQuirks);
        let snapshots = SnapshotMap::new();
        let nodes = Box::new(NodeTree::new());
        let guard = SharedRwLock::new();
        let nodes_to_id = HashMap::new();

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
            root_node_id: NodeId::default(),
            stylist,
            animations: DocumentAnimationSet::default(),
            snapshots,
            nodes_to_id,
            viewport,
            media_type,
            pending_device_changes: DeviceChanges::empty(),
            style_threading: config.style_threading,
            incremental_layout: config.incremental.unwrap_or(true),
            subdocument_depth: config.subdocument_depth,
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
            hover_hit_node_id: None,
            hover_node_is_text: false,
            last_client_pointer_position: None,
            focus_node_id: None,
            active_node_id: None,
            mousedown_node_id: None,
            has_active_animations: false,
            subdoc_is_animating: false,
            has_canvas: false,
            sub_document_nodes: HashSet::new(),
            iframe_loads: HashMap::new(),

            #[cfg(feature = "custom-widget")]
            custom_widget_nodes: HashSet::new(),
            #[cfg(feature = "custom-widget")]
            pending_resource_deallocations: Vec::new(),

            changed_nodes: HashSet::new(),
            deferred_construction_nodes: Vec::new(),
            image_cache: HashMap::new(),
            pending_images: HashMap::new(),
            pending_style_image_nodes: Vec::new(),
            pending_critical_resources: HashSet::new(),
            controls_to_form: HashMap::new(),
            net_provider,
            navigation_provider,
            shell_provider,
            html_parser_provider,
            abort_signal: config.abort_signal,
            last_mousedown_time: None,
            mousedown_position: taffy::Point::ZERO,
            click_count: 0,
            drag_mode: DragMode::None,
            hovered_scrollbar: None,
            scrollbar_activity: HashMap::new(),
            scroll_animation: ScrollAnimationState::None,
            text_selection: TextSelection::default(),
        };

        // Initialise document with root Document node
        doc.root_node_id = doc.create_node(NodeData::Document(Box::default()));
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
        let stylo_data = doc.root_node_mut().stylo_element_data_mut();
        *stylo_data.ensure_init_mut() = stylo_element_data;

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

    /// The base url used for resolving linked resources (stylesheets, images, fonts, etc)
    pub fn base_url(&self) -> &Url {
        &self.url
    }

    pub fn guard(&self) -> &SharedRwLock {
        &self.guard
    }

    pub fn tree(&self) -> &NodeTree {
        &self.nodes
    }

    pub fn id(&self) -> usize {
        self.id
    }

    /// Wrapper around [`crate::net::stamped_request`]. Use the free function
    /// when `&self` would conflict with a held `&mut` borrow on a field.
    pub(crate) fn build_request(&self, url: url::Url) -> Request {
        crate::net::stamped_request(url, self.abort_signal.as_ref())
    }

    pub fn favicon_url(&self) -> Option<String> {
        self.tree().iter().find_map(|(_, node)| {
            let data = &node.data;
            if !data.is_element_with_tag_name(&local_name!("link")) {
                return None;
            }
            let rel = data.attr(local_name!("rel"))?;
            if !rel
                .split_ascii_whitespace()
                .any(|v| v.eq_ignore_ascii_case("icon"))
            {
                return None;
            }
            data.attr(local_name!("href")).map(|s| s.to_string())
        })
    }

    pub fn get_node(&self, node_id: NodeId) -> Option<&Node> {
        self.nodes.get(node_id)
    }

    pub fn get_node_mut(&mut self, node_id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(node_id)
    }

    pub fn get_focussed_node_id(&self) -> Option<NodeId> {
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
    pub fn label_bound_input_element(&self, label_node_id: NodeId) -> Option<&Node> {
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
        let Some(is_checked) = el.checkbox_input_checked() else {
            return false;
        };
        let checked = !is_checked;
        el.set_checkbox_input_checked(checked);

        checked
    }

    pub fn toggle_radio(&mut self, radio_set_name: String, target_radio_id: NodeId) {
        let radio_ids: Vec<NodeId> = self
            .nodes
            .iter()
            .filter_map(|(i, node)| {
                let el = node.data.downcast_element()?;
                (el.attr(local_name!("name")) == Some(&radio_set_name)
                    && el.checkbox_input_checked().is_some())
                .then_some(i)
            })
            .collect();

        for i in radio_ids {
            let checked = i == target_radio_id;
            self.snapshot_node_and(i, ElementState::CHECKED, |node| {
                if let Some(el) = node.element_data_mut() {
                    el.set_checkbox_input_checked(checked);
                }
                node.mark_ancestors_dirty();
            });
        }
    }

    /// Toggle the `open` attribute of a `<details>` element, expanding or
    /// collapsing it. This is the default action triggered when the element's
    /// first `<summary>` child is activated.
    pub fn toggle_details_open(&mut self, details_id: NodeId) {
        use crate::qual_name;

        let node = &self.nodes[details_id];
        if !node.data.is_element_with_tag_name(&local_name!("details")) {
            return;
        }
        let is_open = node.data.has_attr(local_name!("open"));

        // Note: HTML attributes are in the empty (null) namespace, so the
        // QualName must not use the html namespace here, else it won't match
        // an `open` attribute created by the HTML parser.
        let mut mutator = self.mutate();
        if is_open {
            mutator.clear_attribute(details_id, qual_name!("open"));
        } else {
            mutator.set_attribute(details_id, qual_name!("open"), "");
        }
        drop(mutator);

        self.shell_provider.request_redraw();
    }

    pub fn set_style_property(&mut self, node_id: NodeId, name: &str, value: &str) {
        let node = &mut self.nodes[node_id];
        let did_change = node.element_data_mut().unwrap().set_style_property(
            name,
            value,
            &self.guard,
            self.url.url_extra_data(),
        );
        if did_change {
            node.set_restyle_hint(RestyleHint::RESTYLE_STYLE_ATTRIBUTE);
        }
    }

    pub fn remove_style_property(&mut self, node_id: NodeId, name: &str) {
        let node = &mut self.nodes[node_id];
        let did_change = node.element_data_mut().unwrap().remove_style_property(
            name,
            &self.guard,
            self.url.url_extra_data(),
        );
        if did_change {
            node.set_restyle_hint(RestyleHint::RESTYLE_STYLE_ATTRIBUTE);
        }
    }

    pub fn sub_document_node_ids(&self) -> Vec<NodeId> {
        self.sub_document_nodes.iter().copied().collect()
    }

    pub fn set_sub_document(&mut self, node_id: NodeId, sub_document: Box<dyn Document>) {
        self.nodes[node_id]
            .element_data_mut()
            .unwrap()
            .set_sub_document(sub_document);
        self.sub_document_nodes.insert(node_id);
    }

    pub fn remove_sub_document(&mut self, node_id: NodeId) {
        self.nodes[node_id]
            .element_data_mut()
            .unwrap()
            .remove_sub_document();
        self.sub_document_nodes.remove(&node_id);
        if let Some(load) = self.iframe_loads.remove(&node_id) {
            load.abort_controller.abort();
        }
    }

    /// Poll all sub-documents (see [`Document::poll`]), allowing them to make progress
    /// on any pending async operations (e.g. JavaScript timers). Hosts which poll a
    /// wrapper around a [`BaseDocument`] should call this from their `poll` implementation.
    ///
    /// Returns `true` if any sub-document reported changes.
    pub fn poll_subdocuments(&mut self, waker: Option<&Waker>) -> bool {
        let mut has_changes = false;
        for node_id in self.sub_document_nodes.iter().copied() {
            let Some(sub_doc) = self
                .nodes
                .get_mut(node_id)
                .and_then(|node| node.subdoc_mut())
            else {
                continue;
            };
            let task_context = waker.map(TaskContext::from_waker);
            has_changes |= sub_doc.poll(task_context);
        }
        has_changes
    }

    #[cfg(feature = "custom-widget")]
    pub fn custom_widget_node_ids(&self) -> Vec<NodeId> {
        self.custom_widget_nodes.iter().copied().collect()
    }

    #[cfg(feature = "custom-widget")]
    pub fn take_pending_resource_deallocations(&mut self) -> Vec<anyrender::ResourceId> {
        std::mem::take(&mut self.pending_resource_deallocations)
    }

    #[cfg(feature = "custom-widget")]
    pub fn set_custom_widget(&mut self, node_id: NodeId, widget: Box<dyn crate::Widget>) {
        self.nodes[node_id]
            .element_data_mut()
            .unwrap()
            .set_custom_widget(widget);
        self.custom_widget_nodes.insert(node_id);
    }

    #[cfg(feature = "custom-widget")]
    pub fn remove_custom_widget(&mut self, node_id: NodeId) {
        let resources_to_deallocate = self.nodes[node_id]
            .element_data_mut()
            .unwrap()
            .remove_custom_widget();
        self.pending_resource_deallocations
            .extend_from_slice(&resources_to_deallocate);
        self.custom_widget_nodes.remove(&node_id);
    }

    pub fn root_node(&self) -> &Node {
        &self.nodes[self.root_node_id]
    }

    pub fn root_node_mut(&mut self) -> &mut Node {
        &mut self.nodes[self.root_node_id]
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

    pub fn create_node(&mut self, node_data: NodeData) -> NodeId {
        let tree_ptr = self.nodes.as_mut() as *mut NodeTree;
        let guard = self.guard.clone();

        let id = self
            .nodes
            .insert_with_key(|id| Node::new(tree_ptr, id, guard, node_data));

        // Mark the new node as changed.
        self.changed_nodes.insert(id);
        id
    }

    /// Remove a node from the node tree, clearing any interaction state
    /// (hover/active/focus/mousedown/selection/drag/scrollbar) that references
    /// it so that stale NodeIds are never dereferenced after the slot is freed.
    pub(crate) fn remove_node_from_tree(&mut self, node_id: NodeId) -> Option<Node> {
        self.clear_interaction_state_for_removed_node(node_id);
        self.nodes.remove(node_id)
    }

    /// The nearest element ancestor of `node_id` that is still in the
    /// document. Used to retarget hover/active state when the node they
    /// reference is removed. Tolerates already-removed ancestors (subtree
    /// teardown proceeds root-first) by giving up and returning `None`.
    fn nearest_surviving_element_ancestor(&self, node_id: NodeId) -> Option<NodeId> {
        let mut current = self.get_node(node_id)?.parent;
        while let Some(id) = current {
            let node = self.get_node(id)?;
            if node.is_element() && node.flags.is_in_document() {
                return Some(id);
            }
            current = node.parent;
        }
        None
    }

    /// Clear any interaction state (hover/active/focus/mousedown/selection/
    /// drag/scrollbar) that references `node_id`, which is being removed from
    /// the document, running the usual teardown steps. `node_id` must still be
    /// present in the slab.
    ///
    /// This matches browser semantics (WebKit `hoveredElementDidDetach` /
    /// `elementInActiveChainDidDetach`, Blink `HoveredElementDetached` /
    /// `ActiveChainNodeDetached`):
    /// - Hover and active retarget to the nearest surviving element ancestor
    ///   as a *transient bridge*: the HOVER/ACTIVE element-state bits along
    ///   the surviving chain stay lit (no one-frame gap in `:hover`/`:active`
    ///   styling), and the subsequent hover diff can unset exactly the right
    ///   bits. Hover is then re-resolved against the pointer position by
    ///   [`Self::refresh_hover`] at the end of the next resolve pass (the
    ///   analogue of WebKit's "fake mouse move"), which corrects the bridge
    ///   value — including cases where the removed node overflowed its
    ///   ancestor's box, so the ancestor was never truly under the pointer.
    /// - Focus resets to the body (encoded as `None`), running blur
    ///   side-effects (clearing focus element state and disabling IME for
    ///   text inputs).
    pub(crate) fn clear_interaction_state_for_removed_node(&mut self, node_id: NodeId) {
        if !self.nodes.contains_key(node_id) {
            return;
        }

        if self.hover_node_id == Some(node_id) {
            self.hover_node_id = self.nearest_surviving_element_ancestor(node_id);
            self.hover_node_is_text = false;
        }
        if self.hover_hit_node_id == Some(node_id) {
            self.hover_hit_node_id = None;
        }
        if self.active_node_id == Some(node_id) {
            self.active_node_id = self.nearest_surviving_element_ancestor(node_id);
        }
        if self.focus_node_id == Some(node_id) {
            let shell_provider = self.shell_provider.clone();
            self.nodes[node_id].blur(shell_provider);
            self.focus_node_id = None;
        }
        if self.mousedown_node_id == Some(node_id) {
            self.mousedown_node_id = None;
        }
        if self.text_selection.anchor.node_or_parent == Some(node_id)
            || self.text_selection.focus.node_or_parent == Some(node_id)
        {
            self.text_selection.clear();
        }
        if self
            .hovered_scrollbar
            .is_some_and(|scrollbar| scrollbar.node_id == node_id)
        {
            self.hovered_scrollbar = None;
        }
        let drag_references_node = match &self.drag_mode {
            DragMode::Panning(state) => state.target == node_id,
            DragMode::ScrollbarDrag(state) => state.scrollbar.node_id == node_id,
            DragMode::Selecting | DragMode::None => false,
        };
        if drag_references_node {
            self.drag_mode = DragMode::None;
        }
        self.scrollbar_activity.remove(&node_id);
    }

    pub(crate) fn drop_node_ignoring_parent(&mut self, node_id: NodeId) -> Option<Node> {
        self.drop_node_ignoring_parent_with(node_id, &mut |_| {})
    }

    /// Like [`Self::drop_node_ignoring_parent`], but calls `on_drop` with the id of
    /// every dropped node (the node itself and all of its descendants).
    pub(crate) fn drop_node_ignoring_parent_with(
        &mut self,
        node_id: NodeId,
        on_drop: &mut dyn FnMut(NodeId),
    ) -> Option<Node> {
        let mut node = self.remove_node_from_tree(node_id);
        if let Some(node) = &mut node {
            on_drop(node_id);
            if let Some(before) = node.before() {
                self.drop_node_ignoring_parent_with(before, on_drop);
            }
            if let Some(after) = node.after() {
                self.drop_node_ignoring_parent_with(after, on_drop);
            }

            for &child in &node.children {
                self.drop_node_ignoring_parent_with(child, on_drop);
            }

            // Anonymous blocks live only in the slab, so deallocate the ones this
            // node owns rather than leaking them.
            for &anon_id in &node.anonymous_blocks {
                self.deallocate_anonymous_block(anon_id);
            }
        }
        node
    }

    /// Deallocate an anonymous block created in a previous construction
    /// round, along with any anonymous blocks nested within it.
    pub(crate) fn deallocate_anonymous_block(&mut self, anon_id: NodeId) {
        // The block may already have been removed from the slab (e.g. a
        // whitespace-only anonymous block dropped during construction).
        if !self.nodes.contains_key(anon_id) {
            return;
        }

        // Free any anonymous blocks that this block owns before removing it.
        let nested = std::mem::take(&mut self.nodes[anon_id].anonymous_blocks);
        for nested_id in nested {
            self.deallocate_anonymous_block(nested_id);
        }

        self.remove_node_from_tree(anon_id);
    }

    /// Whether the document has been mutated
    pub fn has_changes(&self) -> bool {
        self.changed_nodes.is_empty()
    }

    pub fn create_text_node(&mut self, text: &str) -> NodeId {
        let content = text.to_string();
        let data = NodeData::Text(TextNodeData::new(content));
        self.create_node(data)
    }

    pub fn deep_clone_node(&mut self, node_id: NodeId) -> NodeId {
        // Load existing node
        let node = &self.nodes[node_id];
        let mut data = node.data.clone();

        match &mut data {
            NodeData::Element(elem) | NodeData::AnonymousBlock(elem) => {
                if let Some(arc) = elem.style_attribute.as_mut() {
                    let read_guard = self.guard().read();
                    let block = arc.read_with(&read_guard);
                    *arc = ServoArc::new(self.guard().wrap(block.clone()));
                }
            }
            _ => {}
        }

        let children = node.children.clone();

        // Create new node
        let new_node_id = self.create_node(data);

        // Recursively clone children
        let new_children: ThinVec<NodeId> = children
            .into_iter()
            .map(|child_id| self.deep_clone_node(child_id))
            .collect();
        for &child_id in &new_children {
            self.nodes[child_id].parent = Some(new_node_id);
        }
        self.nodes[new_node_id].children = new_children;

        new_node_id
    }

    pub(crate) fn remove_and_drop_pe(&mut self, node_id: NodeId) -> Option<Node> {
        fn remove_pe_ignoring_parent(doc: &mut BaseDocument, node_id: NodeId) -> Option<Node> {
            let mut node = doc.remove_node_from_tree(node_id);
            if let Some(node) = &mut node {
                for &child in &node.children {
                    remove_pe_ignoring_parent(doc, child);
                }
                for &anon_id in &node.anonymous_blocks {
                    doc.deallocate_anonymous_block(anon_id);
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

    pub fn print_subtree(&self, node_id: NodeId) {
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
                            self.build_request(resolved_href.clone()),
                            ResourceHandler::boxed(
                                self.tx.clone(),
                                self.id,
                                Some(node_id),
                                self.shell_provider.clone(),
                                StylesheetHandler {
                                    source_url: resolved_href,
                                    guard: self.guard.clone(),
                                    net_provider: self.net_provider.clone(),
                                    abort_signal: self.abort_signal.clone(),
                                },
                            ),
                        );
                    }
                }
            }
        }
    }

    pub fn process_style_element(&mut self, target_id: NodeId) {
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

    /// The document's base URL
    pub fn url(&self) -> &url::Url {
        &self.url
    }

    /// Iterate over the author stylesheets (from `<style>` and `<link>` nodes)
    /// currently associated with this document
    pub fn author_stylesheets(&self) -> impl Iterator<Item = &DocumentStyleSheet> {
        self.nodes_to_stylesheet.values()
    }

    /// Iterate over the user-agent stylesheets currently associated with this document
    pub fn useragent_stylesheets(&self) -> impl Iterator<Item = &DocumentStyleSheet> {
        self.ua_stylesheets.values()
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
                abort_signal: self.abort_signal.clone(),
            }),
            None,
            QuirksMode::NoQuirks,
            AllowImportRules::Yes,
        );

        DocumentStyleSheet(ServoArc::new(data))
    }

    pub fn upsert_stylesheet_for_node(&mut self, node_id: NodeId) {
        let raw_styles = self.nodes[node_id].text_content();
        let sheet = self.make_stylesheet(raw_styles, Origin::Author);
        self.add_stylesheet_for_node(sheet, node_id);
    }

    pub fn add_stylesheet_for_node(&mut self, stylesheet: DocumentStyleSheet, node_id: NodeId) {
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
            self.abort_signal.as_ref(),
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
            DocumentEvent::NavigateIframe { node_id, url } => self.navigate_iframe(node_id, url),
        }
    }

    /// Whether the Document has pending requests for "critical" resources (that should block rendering)
    pub fn has_pending_critical_resources(&self) -> bool {
        !self.pending_critical_resources.is_empty()
    }

    pub fn load_resource(&mut self, res: ResourceLoadResponse) {
        self.pending_critical_resources.remove(&res.request_id);

        let resource = match res.result {
            Ok(resource) => resource,
            Err(err) => {
                if let Some(url) = res.resolved_url.as_ref() {
                    let waiting_nodes = self.pending_images.remove(url).unwrap_or_default();
                    #[cfg(feature = "tracing")]
                    tracing::warn!(
                        url = url.as_str(),
                        waiting_nodes = waiting_nodes.len(),
                        error = err.as_str(),
                        "Resource load failed"
                    );
                    #[cfg(not(feature = "tracing"))]
                    let _ = (waiting_nodes, err);
                } else {
                    #[cfg(feature = "tracing")]
                    tracing::warn!(error = err.as_str(), "Resource load failed (no url)");
                    #[cfg(not(feature = "tracing"))]
                    let _ = err;
                }
                return;
            }
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

                self.apply_loaded_image(url, image);
            }
            #[cfg(feature = "svg")]
            Resource::Svg(_kind, svg) => {
                // Create the ImageData and cache it
                let image = ImageData::Svg(svg);

                let Some(url) = res.resolved_url.as_ref() else {
                    return;
                };

                self.apply_loaded_image(url, image);
            }
            Resource::DocumentSrc(html) => {
                let Some(node_id) = res.node_id else {
                    return;
                };
                self.apply_iframe_html(node_id, res.request_id, res.resolved_url, &html);
            }
            Resource::Font(bytes, overrides) => {
                let font = Blob::new(Arc::new(bytes));

                // Build a `FontInfoOverride` from the `@font-face` descriptors
                // captured during stylesheet parsing. Without this, parley
                // reads the family name from the TTF's own metadata, which
                // means CSS `font-family: 'Avenir Book'` won't match a font
                // file that internally identifies as `Avenir 45 Book`.
                let weight_override = overrides.weight.map(parley::fontique::FontWeight::new);
                let info_override = parley::fontique::FontInfoOverride {
                    family_name: overrides.family_name.as_deref(),
                    weight: weight_override,
                    style: overrides.style,
                    ..Default::default()
                };

                // TODO: Investigate eliminating double-box
                let mut global_font_ctx = self.font_ctx.lock().unwrap();
                global_font_ctx
                    .collection
                    .register_fonts(font.clone(), Some(info_override));

                #[cfg(feature = "parallel-construct")]
                {
                    rayon::broadcast(|_ctx| {
                        let mut font_ctx = self
                            .thread_font_contexts
                            .get_or(|| RefCell::new(Box::new(global_font_ctx.clone())))
                            .borrow_mut();
                        font_ctx
                            .collection
                            .register_fonts(font.clone(), Some(info_override));
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

    /// Cache a loaded image and apply it to all nodes waiting on it
    /// (`<img>` elements, `background-image` layers and `mask-image` layers).
    fn apply_loaded_image(&mut self, url: &str, image: ImageData) {
        // Get all nodes waiting for this image
        let waiting_nodes = self.pending_images.remove(url).unwrap_or_default();

        #[cfg(feature = "tracing")]
        tracing::info!(
            "Image {url} loaded, applying to {} nodes",
            waiting_nodes.len()
        );

        // Cache the image
        self.image_cache.insert(url.to_string(), image.clone());

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
                    node.clear_layout_cache();
                    node.insert_damage(ALL_DAMAGE);
                }
                ImageType::Background(idx) | ImageType::Mask(idx) => {
                    let layer_image = node.element_data_mut().and_then(|el| {
                        let images = match image_type {
                            ImageType::Background(_) => &mut el.background_images,
                            ImageType::Mask(_) => &mut el.mask_images,
                            ImageType::Image => unreachable!(),
                        };
                        images.get_mut(idx)
                    });
                    if let Some(Some(layer_image)) = layer_image {
                        layer_image.status = Status::Ok;
                        layer_image.image = image.clone();
                    }
                }
            }
        }
    }

    /// Snapshot the node's pre-mutation state (element state and attributes) ahead of
    /// an attribute mutation, so that the next style traversal can diff selector matches
    /// then-vs-now and invalidate the affected elements.
    pub fn snapshot_node(&mut self, node_id: NodeId) {
        self.snapshot_node_impl(node_id, true)
    }

    /// Snapshot only the node's pre-change [`ElementState`] ahead of a state change
    /// (hover/focus/active/etc). Cheaper than [`Self::snapshot_node`] as it does not
    /// copy attributes or trigger attribute/class/id invalidation work.
    pub fn snapshot_node_state_only(&mut self, node_id: NodeId) {
        self.snapshot_node_impl(node_id, false)
    }

    fn snapshot_node_impl(&mut self, node_id: NodeId, capture_attrs: bool) {
        let node = &mut self.nodes[node_id];

        // Do not snapshot nodes that have never been styled. A snapshot records an element's
        // pre-mutation state so a restyle can diff selector matches then-vs-now. An element
        // that has never been styled has no "then" to diff against. Snapshotting it anyway
        // makes Stylo's invalidation unwrap its (absent) primary style and panic.
        let has_been_styled = node.primary_styles().is_some();
        if !has_been_styled {
            return;
        }

        let opaque_node_id = TNode::opaque(&&*node);
        node.set_has_snapshot(true);
        node.snapshot_handled()
            .store(false, std::sync::atomic::Ordering::SeqCst);

        // A snapshot records the element's state/attributes as they were *before the first
        // mutation* since the last style flush (matching Gecko's ServoElementSnapshot
        // semantics): state is captured at most once, and attributes are captured at most
        // once, but a state-only snapshot is upgraded to also capture attributes if an
        // attribute mutation follows.
        let needs_attrs = capture_attrs
            && self
                .snapshots
                .get_mut(&opaque_node_id)
                .is_none_or(|snapshot| snapshot.attrs.is_none());

        let (attrs, changed_attrs) = if needs_attrs {
            let node = &self.nodes[node_id];
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
                            AttrValue::TokenList(OnceLock::from(attr.value.clone()), classes)
                        } else {
                            AttrValue::String(attr.value.clone())
                        };

                        (ident, value)
                    })
                    .collect()
            });

            let changed_attrs: Vec<_> = attrs
                .as_ref()
                .map(|attrs| attrs.iter().map(|attr| attr.0.name.clone()).collect())
                .unwrap_or_default();

            (attrs, changed_attrs)
        } else {
            (None, Vec::new())
        };

        if let Some(snapshot) = self.snapshots.get_mut(&opaque_node_id) {
            // The existing snapshot's state is preserved: it records the state before
            // the *first* change since the last style flush.
            if needs_attrs {
                snapshot.attrs = attrs;
                snapshot.changed_attrs = changed_attrs;
                snapshot.class_changed = true;
                snapshot.id_changed = true;
                snapshot.other_attributes_changed = true;
            }
        } else {
            self.snapshots.insert(
                opaque_node_id,
                ServoElementSnapshot {
                    state: Some(*self.nodes[node_id].element_state()),
                    attrs,
                    changed_attrs,
                    class_changed: needs_attrs,
                    id_changed: needs_attrs,
                    other_attributes_changed: needs_attrs,
                },
            );
        }
    }

    /// Returns whether any style rule depends on any of the given [`ElementState`] bits.
    /// If not, changing those bits cannot affect styling and snapshotting can be skipped.
    pub fn style_depends_on_state(&self, state: ElementState) -> bool {
        self.stylist.iter_origins().any(|(data, _)| {
            data.has_state_dependency(state) || data.has_nth_of_state_dependency(state)
        })
    }

    /// Apply a state change (hover/focus/active/etc affecting the given [`ElementState`]
    /// bits) to a node, taking a state-only snapshot beforehand if any style rule
    /// depends on those bits.
    pub fn snapshot_node_and(
        &mut self,
        node_id: NodeId,
        state: ElementState,
        cb: impl FnOnce(&mut Node),
    ) {
        if self.style_depends_on_state(state) {
            self.snapshot_node_state_only(node_id);
        }
        cb(&mut self.nodes[node_id]);
    }

    // Takes (x, y) co-ordinates (relative to the )
    pub fn hit(&self, x: f32, y: f32) -> Option<HitResult> {
        self.hit_with_scrollbar(x, y).0
    }

    /// The topmost element at viewport coordinates (x, y), or `None` if the point
    /// is outside the viewport. Anonymous boxes and text nodes are resolved to
    /// their nearest element; a point over the background hits the root element.
    ///
    /// This implements the hit-testing semantics of `document.elementFromPoint()`.
    /// Hit testing consults layout, so [`resolve`](Self::resolve) should be called
    /// before this method to ensure layout is up to date.
    pub fn element_from_point(&self, x: f32, y: f32) -> Option<NodeId> {
        let viewport = self.viewport();
        let scale = viewport.scale();
        let (viewport_width, viewport_height) = (
            viewport.window_size.0 as f32 / scale,
            viewport.window_size.1 as f32 / scale,
        );
        if x < 0.0 || y < 0.0 || x > viewport_width || y > viewport_height {
            return None;
        }

        let Some(hit) = self.hit(x, y) else {
            // The point is within the viewport but over no box: the root element
            // (which covers the viewport in an HTML document) is the hit target
            return self.try_root_element().map(|root| root.id);
        };

        // Resolve anonymous boxes / pseudo-elements, then text nodes, to elements
        let mut node_id = self.nearest_non_anonymous_ancestor(hit.node_id)?;
        loop {
            let node = self.get_node(node_id)?;
            if node.is_element() {
                return Some(node_id);
            }
            node_id = node.parent?;
        }
    }

    /// All elements at viewport coordinates (x, y), from topmost to bottommost,
    /// as for `document.elementsFromPoint()`.
    ///
    /// The spec's paint-order list is approximated with the hit element followed
    /// by its ancestor elements (which is correct for non-overlapping content).
    /// As with [`element_from_point`](Self::element_from_point), layout should be
    /// resolved before calling this method.
    pub fn elements_from_point(&self, x: f32, y: f32) -> Vec<NodeId> {
        let mut element_ids = Vec::new();
        let mut current = self.element_from_point(x, y);
        while let Some(node_id) = current {
            element_ids.push(node_id);
            current = self
                .get_node(node_id)
                .and_then(|node| node.parent)
                .filter(|parent_id| {
                    self.get_node(*parent_id)
                        .is_some_and(|node| node.is_element())
                });
        }
        element_ids
    }

    /// Walk up the tree to the nearest DOM node whose id is stable across
    /// box-tree reconstruction, so canonicalized interaction state never goes
    /// stale.
    ///
    /// Layout-generated nodes (anonymous blocks and `::before`/`::after`
    /// pseudo-elements, both stored as anonymous blocks) get new ids on every
    /// reconstruction, so we skip any anonymous node *and* a non-anonymous node
    /// whose parent is anonymous (the pseudo's text content). The first
    /// non-anonymous node with a non-anonymous parent is a real DOM node; the
    /// root element's `Document` parent guarantees termination.
    ///
    /// Returns `None` if `node_id` (or an ancestor) no longer exists.
    pub fn nearest_non_anonymous_ancestor(&self, node_id: NodeId) -> Option<NodeId> {
        // Recurse up the tree keeping a window of the current node and its
        // parent, advancing one step per iteration so each node is looked up
        // exactly once.
        let mut node = self.get_node(node_id)?;
        loop {
            let parent = match node.parent {
                Some(parent_id) => self.get_node(parent_id)?,
                None => return Some(node.id),
            };
            if !node.is_anonymous() && !parent.is_anonymous() {
                return Some(node.id);
            }
            node = parent;
        }
    }

    pub fn focus_next_node(&mut self) -> Option<NodeId> {
        let focussed_node_id = self.get_focussed_node_id()?;
        let id = self.next_node(&self.nodes[focussed_node_id], |node| node.is_focussable())?;
        self.set_focus_to(id);
        Some(id)
    }

    /// Move focus to the previous focussable node in the document
    pub fn focus_prev_node(&mut self) -> Option<NodeId> {
        let focussed_node_id = self.get_focussed_node_id()?;
        let id = self.prev_node(&self.nodes[focussed_node_id], |node| node.is_focussable())?;
        self.set_focus_to(id);
        Some(id)
    }

    /// Clear the focussed node
    pub fn clear_focus(&mut self) {
        if let Some(id) = self.focus_node_id {
            let shell_provider = self.shell_provider.clone();
            self.snapshot_node_and(id, ElementState::FOCUS | ElementState::FOCUSRING, |node| {
                node.blur(shell_provider)
            });
            self.focus_node_id = None;
        }
    }

    pub fn set_mousedown_node_id(&mut self, node_id: Option<NodeId>) {
        self.mousedown_node_id = node_id.and_then(|id| self.nearest_non_anonymous_ancestor(id));
    }
    pub fn set_focus_to(&mut self, focus_node_id: NodeId) -> bool {
        let Some(focus_node_id) = self.nearest_non_anonymous_ancestor(focus_node_id) else {
            return false;
        };
        if Some(focus_node_id) == self.focus_node_id {
            return false;
        }

        #[cfg(feature = "tracing")]
        tracing::info!("Focussed node {focus_node_id}");

        let shell_provider = self.shell_provider.clone();

        // Remove focus from the old node
        if let Some(id) = self.focus_node_id {
            self.snapshot_node_and(id, ElementState::FOCUS | ElementState::FOCUSRING, |node| {
                node.blur(shell_provider.clone())
            });
        }

        // Focus the new node
        self.snapshot_node_and(
            focus_node_id,
            ElementState::FOCUS | ElementState::FOCUSRING,
            |node| node.focus(shell_provider),
        );

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

        // hover_node_id is canonicalized when stored, so this always holds.
        debug_assert!(
            self.get_node(hover_node_id)
                .is_some_and(|node| !node.is_anonymous()),
            "interaction state must reference DOM nodes, not layout-generated nodes"
        );
        let active_node_id = Some(hover_node_id);

        let node_path = self.maybe_node_layout_ancestors(active_node_id);
        for &id in node_path.iter() {
            self.snapshot_node_and(id, ElementState::ACTIVE, |node| node.active());
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
            self.snapshot_node_and(id, ElementState::ACTIVE, |node| node.unactive());
        }

        true
    }

    /// The scrollbar thumb currently under the pointer, if any.
    pub fn hovered_scrollbar(&self) -> Option<crate::node::ScrollbarRef> {
        self.hovered_scrollbar
    }

    /// The scrollbar thumb currently being dragged, if any.
    pub fn scrollbar_drag_target(&self) -> Option<crate::node::ScrollbarRef> {
        match &self.drag_mode {
            DragMode::ScrollbarDrag(state) => Some(state.scrollbar),
            _ => None,
        }
    }

    /// The current opacity of `node_id`'s overlay scrollbars. They show at
    /// full opacity on scroll and fade out after a delay (Chromium's overlay
    /// timings); the pointer resting on a thumb, or dragging it, holds them
    /// visible.
    pub fn scrollbar_opacity(&self, node_id: NodeId) -> f32 {
        let interacting = |scrollbar: &crate::node::ScrollbarRef| scrollbar.node_id == node_id;
        if self.hovered_scrollbar.as_ref().is_some_and(interacting)
            || self
                .scrollbar_drag_target()
                .as_ref()
                .is_some_and(interacting)
        {
            return 1.0;
        }
        self.scrollbar_activity.get(&node_id).map_or(0.0, |last| {
            crate::node::scrollbar::opacity_at(last.elapsed())
        })
    }

    /// Show `node_id`'s overlay scrollbars at full opacity and restart their
    /// fade-out delay.
    pub(crate) fn show_scrollbars(&mut self, node_id: NodeId) {
        if cfg!(feature = "scrollbars") {
            self.scrollbar_activity.insert(node_id, Instant::now());
        }
    }

    /// Whether any overlay scrollbars are awaiting or animating their
    /// fade-out (so frames must keep rendering until they finish).
    fn scrollbars_animating(&self) -> bool {
        use crate::node::scrollbar::{FADE_DELAY, FADE_DURATION};
        self.scrollbar_activity
            .values()
            .any(|last| last.elapsed() < FADE_DELAY + FADE_DURATION)
    }

    /// [`hit`](Self::hit), also resolving the innermost overlay scrollbar
    /// thumb under the point (shares the traversal, so it costs nothing
    /// extra).
    pub(crate) fn hit_with_scrollbar(
        &self,
        x: f32,
        y: f32,
    ) -> (Option<HitResult>, Option<crate::node::ScrollbarRef>) {
        if TDocument::as_node(&self.root_node())
            .first_element_child()
            .is_none()
        {
            #[cfg(feature = "tracing")]
            tracing::warn!("No DOM - not resolving hit test");
            return (None, None);
        }
        let mut scrollbar = None;
        let hit = self
            .root_element()
            .hit_inner(x, y, self.viewport().scale_f64(), &mut scrollbar);
        (hit, scrollbar)
    }

    pub fn set_hover_to(&mut self, x: f32, y: f32) -> bool {
        // Record the pointer position in client (unscrolled) coordinates so
        // that `refresh_hover` can re-resolve hover state after layout or
        // scroll changes.
        self.last_client_pointer_position = Some(taffy::Point {
            x: x - self.viewport_scroll.x as f32,
            y: y - self.viewport_scroll.y as f32,
        });

        let (hit, hovered_scrollbar) = self.hit_with_scrollbar(x, y);
        // A faded-out thumb is not interactive: pointer moves never fade
        // overlay scrollbars back in (only scrolling shows them).
        let hovered_scrollbar =
            hovered_scrollbar.filter(|scrollbar| self.scrollbar_opacity(scrollbar.node_id) > 0.0);
        // Scrollbar-thumb hover is part of hover state: track it here so a
        // pointer crossing a thumb restyles it even when the hit node (the
        // content under the overlay thumb) is unchanged.
        let scrollbar_changed = hovered_scrollbar != self.hovered_scrollbar;
        if scrollbar_changed {
            // Entering a thumb restores full opacity mid-fade; leaving one
            // restarts the fade-out delay.
            for scrollbar in [self.hovered_scrollbar, hovered_scrollbar]
                .into_iter()
                .flatten()
            {
                self.show_scrollbars(scrollbar.node_id);
            }
        }
        self.hovered_scrollbar = hovered_scrollbar;

        // Store both the precise layout node that was hit (transient: used for
        // cursor/style queries) and its canonical DOM target (persistent: must
        // not reference layout-generated nodes, whose ids die on box-tree
        // reconstruction).
        let hit_node_id = hit.map(|hit| hit.node_id);
        let hover_node_id = hit_node_id.and_then(|id| self.nearest_non_anonymous_ancestor(id));
        let new_is_text = hit.map(|hit| hit.is_text).unwrap_or(false);

        let hit_changed =
            hit_node_id != self.hover_hit_node_id || new_is_text != self.hover_node_is_text;
        self.hover_hit_node_id = hit_node_id;
        self.hover_node_is_text = new_is_text;

        // Return early if the new node is the same as the already-hovered node
        if hover_node_id == self.hover_node_id {
            if hit_changed {
                // The canonical target is unchanged (so no restyle is needed)
                // but the precise hit node changed, which can change the cursor
                // (e.g. moving between text and non-text within one element).
                self.shell_provider.set_cursor(self.get_cursor());
            }
            return scrollbar_changed;
        }

        let old_node_path = self.maybe_node_layout_ancestors(self.hover_node_id);
        let new_node_path = self.maybe_node_layout_ancestors(hover_node_id);
        let same_count = old_node_path
            .iter()
            .zip(&new_node_path)
            .take_while(|(o, n)| o == n)
            .count();
        for &id in old_node_path.iter().skip(same_count) {
            self.snapshot_node_and(id, ElementState::HOVER, |node| node.unhover());
        }
        for &id in new_node_path.iter().skip(same_count) {
            self.snapshot_node_and(id, ElementState::HOVER, |node| node.hover());
        }

        self.hover_node_id = hover_node_id;

        // Update the cursor
        self.shell_provider.set_cursor(self.get_cursor());

        // Request redraw
        self.shell_provider.request_redraw();

        true
    }

    pub fn clear_hover(&mut self) -> bool {
        // The pointer is no longer over the document, so stop re-resolving
        // hover state against it.
        self.last_client_pointer_position = None;
        self.hover_hit_node_id = None;

        let Some(hover_node_id) = self.hover_node_id else {
            return false;
        };

        let old_node_path = self.maybe_node_layout_ancestors(Some(hover_node_id));
        for &id in old_node_path.iter() {
            self.snapshot_node_and(id, ElementState::HOVER, |node| node.unhover());
        }

        self.hover_node_id = None;
        self.hover_node_is_text = false;

        // Update the cursor
        self.shell_provider.set_cursor(self.get_cursor());

        // Request redraw
        self.shell_provider.request_redraw();

        true
    }

    /// Re-resolve hover state against the current layout using the last known
    /// pointer position.
    ///
    /// TODO: synthesizing pointerenter/pointerleave DOM events for
    /// hover changes caused by layout shifts.
    pub fn refresh_hover(&mut self) -> bool {
        let Some(pos) = self.last_client_pointer_position else {
            return false;
        };
        let x = pos.x + self.viewport_scroll.x as f32;
        let y = pos.y + self.viewport_scroll.y as f32;
        self.set_hover_to(x, y)
    }

    pub fn get_hover_node_id(&self) -> Option<NodeId> {
        self.hover_node_id
    }

    pub fn get_mousedown_node_id(&self) -> Option<NodeId> {
        self.mousedown_node_id
    }

    pub fn set_viewport(&mut self, viewport: Viewport) {
        let changes = DeviceChanges::from_viewports(&self.viewport, &viewport);
        self.viewport = viewport;
        self.queue_device_changes(changes);
    }

    /// Returns the current CSS media type used to evaluate `@media` rules.
    pub fn media_type(&self) -> &MediaType {
        &self.media_type
    }

    /// Sets the CSS media type used to evaluate `@media` rules (e.g. `screen` or `print`)
    /// and queues a stylist device rebuild so updated rules apply on the next restyle.
    pub fn set_media_type(&mut self, media_type: MediaType) {
        if self.media_type == media_type {
            return;
        }
        self.media_type = media_type;
        self.queue_device_changes(DeviceChanges::MEDIA_TYPE);
    }

    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    pub fn viewport_mut(&mut self) -> ViewportMut<'_> {
        ViewportMut::new(self)
    }

    pub fn zoom_by(&mut self, increment: f32) {
        *self.viewport_mut().zoom_mut() += increment;
    }

    pub fn zoom_to(&mut self, zoom: f32) {
        *self.viewport_mut().zoom_mut() = zoom;
    }

    pub fn get_viewport(&self) -> Viewport {
        self.viewport.clone()
    }

    /// Returns whether incremental layout is currently enabled for this document.
    pub fn incremental_layout(&self) -> bool {
        self.incremental_layout
    }

    /// Enables or disables incremental layout for this document.
    pub fn set_incremental_layout(&mut self, enabled: bool) {
        self.incremental_layout = enabled;
    }

    pub fn devtools(&self) -> &DevtoolSettings {
        &self.devtool_settings
    }

    pub fn devtools_mut(&mut self) -> &mut DevtoolSettings {
        &mut self.devtool_settings
    }

    pub fn subdoc(&self, node_id: NodeId) -> Option<&dyn Document> {
        self.get_node(node_id)
            .and_then(|node| node.element_data())
            .and_then(|el| el.sub_doc_data())
    }

    pub fn subdoc_mut(&mut self, node_id: NodeId) -> Option<&mut dyn Document> {
        self.get_node_mut(node_id)
            .and_then(|node| node.element_data_mut())
            .and_then(|el| el.sub_doc_data_mut())
    }

    pub fn is_animating(&self) -> bool {
        #[cfg(feature = "custom-widget")]
        let custom_widget_is_animating = self.custom_widget_nodes.iter().any(|&node_id| {
            self.nodes[node_id]
                .element_data()
                .and_then(|el| el.custom_widget_data())
                .is_some_and(|data| data.widget.requires_redraw())
        });
        #[cfg(not(feature = "custom-widget"))]
        let custom_widget_is_animating = false;

        self.has_canvas
            | self.has_active_animations
            | self.subdoc_is_animating
            | custom_widget_is_animating
            | (self.scroll_animation != ScrollAnimationState::None)
            | self.scrollbars_animating()
    }

    /// Record pending [`DeviceChanges`] and request a redraw so that they are
    /// flushed (via [`Self::flush_pending_device_changes`]) on the next resolve.
    pub(crate) fn queue_device_changes(&mut self, changes: DeviceChanges) {
        if changes.is_empty() {
            return;
        }
        self.pending_device_changes |= changes;
        self.shell_provider.request_redraw();
    }

    /// Apply any pending device changes to the stylist, coalescing all changes
    /// since the last flush into a single device rebuild.
    pub(crate) fn flush_pending_device_changes(&mut self) {
        let changes = std::mem::take(&mut self.pending_device_changes);
        if changes.is_empty() {
            return;
        }

        self.set_stylist_device(make_device(
            &self.viewport,
            self.media_type.clone(),
            self.font_ctx.clone(),
        ));
        self.scroll_viewport_by(0.0, 0.0); // Clamp scroll offset

        // Text is shaped at a specific scale factor, so cached inline layouts
        // must be invalidated when the scale changes.
        if changes.contains(DeviceChanges::SCALE) {
            self.invalidate_inline_contexts();
        }

        // Color-scheme changes affect values that are resolved at cascade time
        // (`light-dark()`, system colors) without necessarily flipping any
        // media query result, so conservatively recascade the whole tree.
        if changes.contains(DeviceChanges::COLOR_SCHEME) {
            if let Some(root_id) = self.try_root_element().map(|el| el.id) {
                self.nodes[root_id].set_restyle_hint(RestyleHint::recascade_subtree());
            }
        }
    }

    /// Update the device and reset the stylist to process the new size
    pub fn set_stylist_device(&mut self, device: Device) {
        // Seed the new device with the root element's current style and font-relative
        // unit state (used to resolve rem/rlh/rex/rch/rcap/ric units). Stylo only
        // updates this state when the root element's style *changes* during a restyle,
        // so a freshly-built device would otherwise resolve these units against the
        // default font-size (16px) until the root's font-size next changes.
        let root_styles = self
            .try_root_element()
            .and_then(|root| root.primary_styles());
        if let Some(root_style) = root_styles.as_deref() {
            device.set_root_style(root_style);

            let font = root_style.get_font();
            let font_size = font.clone_font_size().computed_size();
            device.set_root_font_size(root_style.effective_zoom.unzoom(font_size.px()));

            let line_height = device
                .calc_line_height(font, root_style.writing_mode, None)
                .0;
            device.set_root_line_height(root_style.effective_zoom.unzoom(line_height.px()));
        }
        drop(root_styles);

        let old_device = self.stylist.device();
        let viewport_size_changed = old_device.au_viewport_size() != device.au_viewport_size();
        let styles_use_viewport_units = old_device.used_viewport_size();

        let origins = {
            let guard = &self.guard;
            let guards = StylesheetGuards {
                author: &guard.read(),
                ua_or_user: &guard.read(),
            };
            self.stylist.set_device(device, &guards)
        };

        // Only fully invalidate element styles when the media query results of
        // some origin actually changed. `force_stylesheet_origins_dirty` fully
        // invalidates styles even when `origins` is empty, which would force a
        // full restyle on every viewport resize.
        if !origins.is_empty() {
            self.stylist.force_stylesheet_origins_dirty(origins);
        }

        // Styles that resolve viewport units (vw/vh/etc) still need recomputing
        // when the viewport size changes, even if no media query results
        // changed. Invalidate just the elements whose styles use viewport
        // units (tracked via per-element computed value flags).
        if viewport_size_changed && styles_use_viewport_units {
            if self
                .stylist
                .get_custom_property_initial_values_flags()
                .intersects(ComputedValueFlags::USES_VIEWPORT_UNITS)
            {
                self.stylist.rebuild_initial_values_for_custom_properties();
            }
            if let Some(root) = self.try_root_element() {
                style::invalidation::viewport_units::invalidate(root);
            }
        }
    }

    pub fn stylist_device(&mut self) -> &Device {
        self.flush_pending_device_changes();
        self.stylist.device()
    }

    pub fn get_cursor(&self) -> Option<CursorIcon> {
        // Prefer the precise hit node: `cursor` and `user-select` may be set on
        // a pseudo-element or resolved on an anonymous box, and text hits carry
        // is_text via the hit node. Fall back to the canonical hover node if
        // the hit node has been removed (it is transient across resolves).
        let node_id = self
            .hover_hit_node_id
            .filter(|&id| self.nodes.contains_key(id))
            .or(self.get_hover_node_id())?;
        let node = &self.nodes[node_id];

        if let Some(subdoc) = node.subdoc().map(|doc| doc.inner()) {
            return subdoc.get_cursor();
        }

        let style = node.primary_styles()?;
        let user_select = style.clone_user_select();
        let keyword = style.clone_cursor().keyword;

        // Return cursor from style if it is non-auto
        if keyword != CursorKind::Auto {
            return stylo_to_cursor_icon(keyword);
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
            return Some(match user_select {
                UserSelect::Text | UserSelect::All | UserSelect::Auto => CursorIcon::Text,
                UserSelect::None => CursorIcon::Default,
            });
        }

        // Else fallback to default cursor
        Some(CursorIcon::Default)
    }

    pub fn viewport_scroll(&self) -> crate::Point<f64> {
        self.viewport_scroll
    }

    pub fn set_viewport_scroll(&mut self, scroll: crate::Point<f64>) {
        self.viewport_scroll = scroll;
    }

    /// Find the node targeted by a URL fragment (the `#...` part of a URL).
    ///
    /// Per the HTML spec, this is the element whose `id` matches the fragment, falling
    /// back to the first `<a>` element whose `name` attribute matches.
    pub fn get_fragment_target(&self, fragment: &str) -> Option<NodeId> {
        if let Some(node_id) = self.get_element_by_id(fragment) {
            return Some(node_id);
        }

        // Fall back to a named anchor: `<a name="...">`
        self.nodes.iter().find_map(|(id, node)| {
            let el = node.element_data()?;
            (el.name.local == local_name!("a") && el.attr(local_name!("name")) == Some(fragment))
                .then_some(id)
        })
    }

    /// Computes the size and position of the `Node` relative to the viewport
    pub fn get_client_bounding_rect(&self, node_id: NodeId) -> Option<BoundingRect> {
        // Non-atomic inline elements have no layout box of their own: return
        // the union of their per-line-box fragment rects.
        if let Some(rects) = self.inline_fragment_rects(node_id) {
            let x0 = rects.iter().map(|r| r.x).fold(f64::INFINITY, f64::min);
            let y0 = rects.iter().map(|r| r.y).fold(f64::INFINITY, f64::min);
            let x1 = rects
                .iter()
                .map(|r| r.x + r.width)
                .fold(f64::NEG_INFINITY, f64::max);
            let y1 = rects
                .iter()
                .map(|r| r.y + r.height)
                .fold(f64::NEG_INFINITY, f64::max);
            return match rects.is_empty() {
                true => None,
                false => Some(BoundingRect {
                    x: x0,
                    y: y0,
                    width: x1 - x0,
                    height: y1 - y0,
                }),
            };
        }

        let node = self.get_node(node_id)?;
        let pos = node.absolute_position(0.0, 0.0);

        Some(BoundingRect {
            x: pos.x as f64 - self.viewport_scroll.x,
            y: pos.y as f64 - self.viewport_scroll.y,
            width: node.unrounded_layout().size.width as f64,
            height: node.unrounded_layout().size.height as f64,
        })
    }

    /// Computes the sizes and positions of the `Node`'s box fragments relative to the
    /// viewport (CSSOM `getClientRects()` semantics). Nodes with their own layout box
    /// return a single rect. Non-atomic inline elements (which are laid out as style
    /// spans within an inline root's text layout) return one rect per line box.
    pub fn node_client_rects(&self, node_id: NodeId) -> Vec<BoundingRect> {
        match self.inline_fragment_rects(node_id) {
            Some(rects) => rects,
            None => self.get_client_bounding_rect(node_id).into_iter().collect(),
        }
    }

    /// Computes per-line-box fragment rects for a non-atomic inline element by walking
    /// the containing inline root's text layout. Returns `None` for nodes that have
    /// their own layout box (which should use `get_client_bounding_rect` instead).
    pub fn inline_fragment_rects(&self, node_id: NodeId) -> Option<Vec<BoundingRect>> {
        use parley::PositionedLayoutItem;

        let node = self.get_node(node_id)?;

        // Only non-atomic inline elements lack their own layout box: they are
        // flattened into the containing inline root's text layout as style spans.
        if !node.is_element() || node.flags.is_inline_root() {
            return None;
        }
        let display = node.primary_styles()?.clone_display();
        if !(display.outside() == DisplayOutside::Inline && display.inside() == DisplayInside::Flow)
        {
            return None;
        }

        let inline_root = node.inline_root_ancestor()?;
        let inline_layout = inline_root.element_data()?.inline_layout_data.as_ref()?;
        let layout = &inline_layout.layout;
        let scale = layout.scale() as f64;

        // Walk up the DOM parent chain from `id` to check whether it is (or is
        // inside) the target node, stopping at the inline root.
        let is_in_target = |mut id: NodeId| -> bool {
            loop {
                if id == node_id {
                    return true;
                }
                if id == inline_root.id {
                    return false;
                }
                match self.get_node(id).and_then(|n| n.parent) {
                    Some(parent) => id = parent,
                    None => return false,
                }
            }
        };

        // Fragment rects are relative to the inline root's content box.
        let root_layout = inline_root.final_layout();
        let root_pos = inline_root.absolute_position(0.0, 0.0);
        let origin_x = root_pos.x as f64
            + (root_layout.padding.left + root_layout.border.left) as f64
            - self.viewport_scroll.x;
        let origin_y = root_pos.y as f64
            + (root_layout.padding.top + root_layout.border.top) as f64
            - self.viewport_scroll.y;

        let mut rects: Vec<BoundingRect> = Vec::new();
        for line in layout.lines() {
            let line_metrics = line.metrics();
            // Union all of the target's fragments on this line into a single rect
            let mut line_rect: Option<(f64, f64, f64, f64)> = None;
            let mut add = |x0: f64, y0: f64, x1: f64, y1: f64| {
                line_rect = Some(match line_rect {
                    Some((lx0, ly0, lx1, ly1)) => {
                        (lx0.min(x0), ly0.min(y0), lx1.max(x1), ly1.max(y1))
                    }
                    None => (x0, y0, x1, y1),
                });
            };

            for item in line.items() {
                match item {
                    PositionedLayoutItem::GlyphRun(glyph_run) => {
                        if !is_in_target(glyph_run.style().brush.id) {
                            continue;
                        }
                        let x0 = glyph_run.offset() as f64;
                        let x1 = x0 + glyph_run.advance() as f64;
                        // Use the line box's block extent rather than the
                        // run's font ascent/descent: fonts with small
                        // typographic metrics would otherwise produce rects
                        // that clip the rendered glyphs. This matches the
                        // geometry used for text selection highlights.
                        let y0 = line_metrics.block_min_coord as f64;
                        let y1 = line_metrics.block_max_coord as f64;
                        add(x0, y0, x1, y1);
                    }
                    PositionedLayoutItem::InlineBox(inline_box) => {
                        if !is_in_target(NodeId::from_u64(inline_box.id)) {
                            continue;
                        }
                        let x0 = inline_box.x as f64;
                        let y0 = inline_box.y as f64;
                        add(
                            x0,
                            y0,
                            x0 + inline_box.width as f64,
                            y0 + inline_box.height as f64,
                        );
                    }
                }
            }

            if let Some((x0, y0, x1, y1)) = line_rect {
                rects.push(BoundingRect {
                    x: origin_x + x0 / scale,
                    y: origin_y + y0 / scale,
                    width: (x1 - x0) / scale,
                    height: (y1 - y0) / scale,
                });
            }
        }

        Some(rects)
    }

    /// The first element in tree order with the given tag name. The root element and
    /// its children are checked first as a fast path before a full tree search, making
    /// this suitable for the `documentElement`/`head`/`body` document accessors.
    pub fn find_element_by_tag_name(&self, tag: &LocalName) -> Option<&Node> {
        let root = self.try_root_element()?;
        if root.data.is_element_with_tag_name(tag) {
            return Some(root);
        }
        root.children
            .iter()
            .copied()
            .find(|child_id| {
                self.get_node(*child_id)
                    .is_some_and(|child| child.data.is_element_with_tag_name(tag))
            })
            .or_else(|| {
                TreeTraverser::new(self)
                    .find(|node_id| self.nodes[*node_id].data.is_element_with_tag_name(tag))
            })
            .map(|node_id| &self.nodes[node_id])
    }

    /// The document's `body` element, as for `document.body`
    pub fn find_body_node(&self) -> Option<&Node> {
        self.find_element_by_tag_name(&local_name!("body"))
    }

    /// The document's `head` element, as for `document.head`
    pub fn find_head_node(&self) -> Option<&Node> {
        self.find_element_by_tag_name(&local_name!("head"))
    }

    /// The document's `title` element
    pub fn find_title_node(&self) -> Option<&Node> {
        self.find_element_by_tag_name(&local_name!("title"))
    }

    pub fn with_text_input(
        &mut self,
        node_id: NodeId,
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

    /// Recompute the scroll offset of the text input at `node_id` (if any) so that its caret
    /// remains visible within the input's content box.
    pub(crate) fn clamp_text_input_scroll(&mut self, node_id: NodeId) {
        let Some(node) = self.nodes.get_mut(node_id) else {
            return;
        };

        let content_box_width = node.final_layout().content_box_width();
        let content_box_height = node.final_layout().content_box_height();

        if let Some(text_input) = node
            .element_data_mut()
            .and_then(|el| el.text_input_data_mut())
        {
            text_input.clamp_scroll_offset(content_box_width, content_box_height);
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
    pub fn find_text_position(&self, x: f32, y: f32) -> Option<(NodeId, usize)> {
        let hit = self.hit(x, y)?;
        let hit_node = self.get_node(hit.node_id)?;
        let inline_root = hit_node.inline_root_ancestor()?;
        let byte_offset = inline_root.text_offset_at_point(hit.x, hit.y)?;
        Some((inline_root.id, byte_offset))
    }

    /// Set the text selection range (creates a new selection from anchor to focus)
    pub fn set_text_selection(
        &mut self,
        anchor_node: NodeId,
        anchor_offset: usize,
        focus_node: NodeId,
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
    fn anonymous_block_location(&self, node_id: NodeId) -> (Option<NodeId>, Option<usize>) {
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
    pub fn update_selection_focus(&mut self, focus_node: NodeId, focus_offset: usize) {
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
        parent_id: NodeId,
        target_index: usize,
    ) -> Option<NodeId> {
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
    pub fn get_text_selection_ranges(&self) -> Vec<(NodeId, usize, usize)> {
        let lookup = |parent_id, idx| self.find_anonymous_block_by_index(parent_id, idx);

        let anchor_node = match self.text_selection.anchor.resolve_node_id(lookup) {
            Some(id) => id,
            None => return Vec::new(),
        };
        let focus_node = match self.text_selection.focus.resolve_node_id(lookup) {
            Some(id) => id,
            None => return Vec::new(),
        };

        // Guard against stale selection endpoints: nodes may have been removed from
        // the document (e.g. by script) since the selection was made.
        let node_is_in_doc = |node_id: NodeId| {
            self.nodes
                .get(node_id)
                .is_some_and(|node| node.flags.is_in_document())
        };
        if !node_is_in_doc(anchor_node) || !node_is_in_doc(focus_node) {
            return Vec::new();
        }

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

#[derive(Debug, Clone, Copy, PartialEq)]
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

#[cfg(test)]
mod zoom_tests {
    use super::*;
    use blitz_traits::shell::ColorScheme;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct CountingShellProvider {
        redraw_count: AtomicUsize,
    }
    impl ShellProvider for CountingShellProvider {
        fn request_redraw(&self) {
            self.redraw_count.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn zoom_requests_redraw() {
        let shell_provider = Arc::new(CountingShellProvider::default());
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            shell_provider: Some(shell_provider.clone() as _),
            ..Default::default()
        });
        doc.resolve(0.0);

        let base = shell_provider.redraw_count.load(Ordering::SeqCst);
        doc.zoom_by(0.5);
        assert!(shell_provider.redraw_count.load(Ordering::SeqCst) > base);

        let base = shell_provider.redraw_count.load(Ordering::SeqCst);
        doc.zoom_to(1.0);
        assert!(shell_provider.redraw_count.load(Ordering::SeqCst) > base);
    }
}

#[cfg(test)]
mod hover_state_tests {
    use super::*;
    use crate::{Attribute, qual_name};
    use blitz_traits::shell::ColorScheme;

    /// Build `<html><body style="margin:0"><div style="width:300px">some text
    /// <div style="height:50px"></div></div></body></html>` manually (the HTML
    /// parser lives in blitz-html, which would be a circular dev-dependency).
    /// The bare text next to a block sibling gets wrapped in an anonymous
    /// block, which becomes the inline root: text hits report the anonymous
    /// block as the hit node.
    fn make_doc() -> (BaseDocument, NodeId) {
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            ..Default::default()
        });
        let root_id = doc.root_node().id;
        let style = |value: &str| Attribute {
            name: qual_name!("style"),
            value: value.to_string(),
        };

        let mut mutator = doc.mutate();
        let html = mutator.create_element(qual_name!("html"), vec![]);
        let body = mutator.create_element(qual_name!("body"), vec![style("margin:0")]);
        let container = mutator.create_element(qual_name!("div"), vec![style("width:300px")]);
        let text = mutator.create_text_node("some text");
        let block = mutator.create_element(qual_name!("div"), vec![style("height:50px")]);
        mutator.append_children(container, &[text, block]);
        mutator.append_children(body, &[container]);
        mutator.append_children(html, &[body]);
        mutator.append_children(root_id, &[html]);
        drop(mutator);

        doc.resolve(0.0);
        (doc, container)
    }

    /// Whether text laid out with a real (non-zero-metric) font. Without the
    /// `system-fonts` feature text measures 0x0 and text hits are impossible,
    /// making these tests vacuous.
    fn text_has_size(doc: &BaseDocument, container: NodeId) -> bool {
        doc.nodes[container].final_layout().size.height > 50.0
    }

    /// Regression test: hovering bare text wrapped in an anonymous block must
    /// report a text cursor. The hit node for such text is the anonymous
    /// inline root itself, while the *stored* hover target is canonicalized to
    /// the containing element — the cursor must be derived from the precise
    /// hit node, not the canonical target.
    #[test]
    fn hovering_text_in_anonymous_block_reports_text_cursor() {
        let (mut doc, container) = make_doc();
        if !text_has_size(&doc, container) {
            eprintln!("skipping: no usable font (text measures 0x0)");
            return;
        }

        doc.set_hover_to(5.0, 8.0);
        assert!(doc.hover_node_is_text, "expected a text hit");
        let hit_id = doc.hover_hit_node_id.expect("expected a hit node");
        assert!(
            doc.nodes[hit_id].is_anonymous(),
            "expected the hit node to be the anonymous inline root"
        );
        assert_eq!(
            doc.get_hover_node_id(),
            Some(container),
            "expected the stored hover target to be the containing element"
        );
        assert_eq!(doc.get_cursor(), Some(CursorIcon::Text));
    }

    /// Hovering the empty region of the anonymous block (right of the text) is
    /// not a text hit: default cursor, same canonical hover target.
    #[test]
    fn hovering_anonymous_block_whitespace_reports_default_cursor() {
        let (mut doc, container) = make_doc();
        if !text_has_size(&doc, container) {
            eprintln!("skipping: no usable font (text measures 0x0)");
            return;
        }

        doc.set_hover_to(250.0, 8.0);
        assert!(!doc.hover_node_is_text);
        assert_eq!(doc.get_hover_node_id(), Some(container));
        assert_eq!(doc.get_cursor(), Some(CursorIcon::Default));
    }
}

#[cfg(test)]
mod hover_invalidation_tests {
    use super::*;
    use crate::{Attribute, QualName, qual_name};
    use blitz_traits::shell::ColorScheme;

    /// Build `<html><body style="margin:0"><div style="width:300px;height:100px">
    /// <span>text</span></div></body></html>` with a `div:hover` rule.
    fn make_doc() -> (BaseDocument, NodeId, NodeId) {
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            ..Default::default()
        });
        doc.add_user_agent_stylesheet(
            "div:hover { background-color: rgb(255, 0, 0); } div:hover span { color: rgb(0, 255, 0); }",
        );
        let root_id = doc.root_node().id;
        let style = |value: &str| Attribute {
            name: qual_name!("style"),
            value: value.to_string(),
        };

        let mut mutator = doc.mutate();
        let html = mutator.create_element(qual_name!("html"), vec![]);
        let body = mutator.create_element(qual_name!("body"), vec![style("margin:0")]);
        let div =
            mutator.create_element(qual_name!("div"), vec![style("width:300px;height:100px")]);
        let span = mutator.create_element(qual_name!("span"), vec![]);
        let text = mutator.create_text_node("some text");
        mutator.append_children(span, &[text]);
        mutator.append_children(div, &[span]);
        mutator.append_children(body, &[div]);
        mutator.append_children(html, &[body]);
        mutator.append_children(root_id, &[html]);
        drop(mutator);

        doc.resolve(0.0);
        (doc, div, span)
    }

    fn bg_color(doc: &BaseDocument, id: NodeId) -> String {
        format!(
            "{:?}",
            doc.nodes[id]
                .primary_styles()
                .unwrap()
                .get_background()
                .background_color
        )
    }

    fn text_color(doc: &BaseDocument, id: NodeId) -> String {
        format!(
            "{:?}",
            doc.nodes[id].primary_styles().unwrap().clone_color()
        )
    }

    #[test]
    fn hover_styles_apply_and_clear() {
        let (mut doc, div, span) = make_doc();
        let initial_bg = bg_color(&doc, div);
        let initial_color = text_color(&doc, span);

        // Hover the div
        doc.set_hover_to(10.0, 10.0);
        assert!(doc.nodes[div].is_hovered());
        doc.resolve(0.0);
        let hovered_bg = bg_color(&doc, div);
        let hovered_color = text_color(&doc, span);
        assert_ne!(initial_bg, hovered_bg, "hover should change div background");
        assert_ne!(
            initial_color, hovered_color,
            "hover should change span color"
        );

        // Move the pointer off the div (below it, over the body)
        doc.set_hover_to(10.0, 200.0);
        assert!(!doc.nodes[div].is_hovered());
        doc.resolve(0.0);
        assert_eq!(
            bg_color(&doc, div),
            initial_bg,
            "unhover should restore div background"
        );
        assert_eq!(
            text_color(&doc, span),
            initial_color,
            "unhover should restore span color"
        );
    }

    /// Mimics BBC's headline hover pattern: `a:link:hover .headline` with the
    /// headline several levels below the anchor.
    #[test]
    fn ancestor_hover_with_link_state_updates_descendant() {
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            ..Default::default()
        });
        doc.add_user_agent_stylesheet(
            ".promo:link:hover .headline, .promo:visited:hover .headline { color: rgb(184, 0, 0); text-decoration-line: underline; }",
        );
        let root_id = doc.root_node().id;
        let attr = |name: QualName, value: &str| Attribute {
            name,
            value: value.to_string(),
        };

        let mut mutator = doc.mutate();
        let html = mutator.create_element(qual_name!("html"), vec![]);
        let body = mutator.create_element(
            qual_name!("body"),
            vec![attr(qual_name!("style"), "margin:0")],
        );
        let a = mutator.create_element(
            qual_name!("a"),
            vec![
                attr(qual_name!("href"), "https://example.com"),
                attr(qual_name!("class"), "promo"),
                attr(
                    qual_name!("style"),
                    "display:block;width:300px;height:100px",
                ),
            ],
        );
        let p = mutator.create_element(qual_name!("p"), vec![]);
        let span = mutator.create_element(
            qual_name!("span"),
            vec![attr(qual_name!("class"), "headline")],
        );
        let text = mutator.create_text_node("Headline text");
        mutator.append_children(span, &[text]);
        mutator.append_children(p, &[span]);
        mutator.append_children(a, &[p]);
        mutator.append_children(body, &[a]);
        mutator.append_children(html, &[body]);
        mutator.append_children(root_id, &[html]);
        drop(mutator);

        doc.resolve(0.0);
        let initial_color = text_color(&doc, span);

        doc.set_hover_to(10.0, 10.0);
        assert!(doc.nodes[a].is_hovered());
        doc.resolve(0.0);
        let hovered_color = text_color(&doc, span);
        assert_ne!(
            initial_color, hovered_color,
            "hovering the anchor should change the headline color"
        );

        doc.set_hover_to(10.0, 200.0);
        assert!(!doc.nodes[a].is_hovered());
        doc.resolve(0.0);
        assert_eq!(
            text_color(&doc, span),
            initial_color,
            "unhovering the anchor should restore the headline color"
        );
    }

    /// Toggling a checkbox must invalidate `:checked`-dependent styles.
    #[test]
    fn checkbox_toggle_updates_checked_styles() {
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            ..Default::default()
        });
        doc.add_user_agent_stylesheet(
            "input:checked { color: rgb(184, 0, 0); } input:checked + label { color: rgb(0, 184, 0); }",
        );
        let root_id = doc.root_node().id;
        let attr = |name: QualName, value: &str| Attribute {
            name,
            value: value.to_string(),
        };

        let mut mutator = doc.mutate();
        let html = mutator.create_element(qual_name!("html"), vec![]);
        let body = mutator.create_element(
            qual_name!("body"),
            vec![attr(qual_name!("style"), "margin:0")],
        );
        let input = mutator.create_element(
            qual_name!("input"),
            vec![attr(qual_name!("type"), "checkbox")],
        );
        let label = mutator.create_element(qual_name!("label"), vec![]);
        let text = mutator.create_text_node("label text");
        mutator.append_children(label, &[text]);
        mutator.append_children(body, &[input, label]);
        mutator.append_children(html, &[body]);
        mutator.append_children(root_id, &[html]);
        drop(mutator);

        doc.resolve(0.0);
        let initial_input_color = text_color(&doc, input);
        let initial_label_color = text_color(&doc, label);

        // Toggle the checkbox on (as the click handler does)
        doc.snapshot_node_and(input, ElementState::CHECKED, |node| {
            if let Some(el) = node.element_data_mut() {
                BaseDocument::toggle_checkbox(el);
            }
            node.mark_ancestors_dirty();
        });
        doc.resolve(0.0);
        assert_ne!(
            text_color(&doc, input),
            initial_input_color,
            "checking should change the input color"
        );
        assert_ne!(
            text_color(&doc, label),
            initial_label_color,
            "checking should change the sibling label color"
        );

        // Toggle the checkbox back off
        doc.snapshot_node_and(input, ElementState::CHECKED, |node| {
            if let Some(el) = node.element_data_mut() {
                BaseDocument::toggle_checkbox(el);
            }
            node.mark_ancestors_dirty();
        });
        doc.resolve(0.0);
        assert_eq!(
            text_color(&doc, input),
            initial_input_color,
            "unchecking should restore the input color"
        );
        assert_eq!(
            text_color(&doc, label),
            initial_label_color,
            "unchecking should restore the sibling label color"
        );
    }

    /// A repaint-only style change to an existing `::before` pseudo-element
    /// (no box construction damage) must be flushed to the pseudo-element's
    /// anonymous node during the style traversal.
    #[test]
    fn hover_updates_existing_pseudo_element_style() {
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            ..Default::default()
        });
        doc.add_user_agent_stylesheet(
            "div::before { content: \"x\"; color: rgb(1, 2, 3); } \
             div:hover::before { color: rgb(0, 0, 255); }",
        );
        let root_id = doc.root_node().id;
        let style = |value: &str| Attribute {
            name: qual_name!("style"),
            value: value.to_string(),
        };

        let mut mutator = doc.mutate();
        let html = mutator.create_element(qual_name!("html"), vec![]);
        let body = mutator.create_element(qual_name!("body"), vec![style("margin:0")]);
        let div = mutator.create_element(
            qual_name!("div"),
            vec![style("display:block;width:300px;height:100px")],
        );
        let text = mutator.create_text_node("some text");
        mutator.append_children(div, &[text]);
        mutator.append_children(body, &[div]);
        mutator.append_children(html, &[body]);
        mutator.append_children(root_id, &[html]);
        drop(mutator);

        doc.resolve(0.0);
        let before = doc.nodes[div].before().expect("::before node should exist");
        let initial_color = text_color(&doc, before);

        doc.set_hover_to(10.0, 10.0);
        assert!(doc.nodes[div].is_hovered());
        doc.resolve(0.0);
        assert_ne!(
            text_color(&doc, before),
            initial_color,
            "hover should change the ::before color"
        );

        doc.set_hover_to(10.0, 200.0);
        assert!(!doc.nodes[div].is_hovered());
        doc.resolve(0.0);
        assert_eq!(
            text_color(&doc, before),
            initial_color,
            "unhover should restore the ::before color"
        );
    }

    /// Background-image layers must be flushed to the node's dedicated image
    /// storage when its style changes (queued during the style traversal),
    /// now that image flushing no longer runs on every node in the flush pass.
    #[test]
    fn hover_updates_background_image_layers() {
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            ..Default::default()
        });
        doc.add_user_agent_stylesheet(
            "div { background-image: url(\"https://example.com/a.png\"); } \
             div:hover { background-image: url(\"https://example.com/b.png\"); }",
        );
        let root_id = doc.root_node().id;
        let style = |value: &str| Attribute {
            name: qual_name!("style"),
            value: value.to_string(),
        };

        let mut mutator = doc.mutate();
        let html = mutator.create_element(qual_name!("html"), vec![]);
        let body = mutator.create_element(qual_name!("body"), vec![style("margin:0")]);
        let div = mutator.create_element(
            qual_name!("div"),
            vec![style("display:block;width:300px;height:100px")],
        );
        mutator.append_children(body, &[div]);
        mutator.append_children(html, &[body]);
        mutator.append_children(root_id, &[html]);
        drop(mutator);

        let background_image_url = |doc: &BaseDocument, id: NodeId| -> Option<String> {
            let elem = doc.nodes[id].data.downcast_element().unwrap();
            elem.background_images
                .first()
                .and_then(|img| img.as_ref())
                .map(|img| img.url.as_str().to_string())
        };

        doc.resolve(0.0);
        assert_eq!(
            background_image_url(&doc, div).as_deref(),
            Some("https://example.com/a.png"),
            "initial resolve should flush the background image"
        );

        doc.set_hover_to(10.0, 10.0);
        assert!(doc.nodes[div].is_hovered());
        doc.resolve(0.0);
        assert_eq!(
            background_image_url(&doc, div).as_deref(),
            Some("https://example.com/b.png"),
            "hover should flush the changed background image"
        );

        doc.set_hover_to(10.0, 200.0);
        assert!(!doc.nodes[div].is_hovered());
        doc.resolve(0.0);
        assert_eq!(
            background_image_url(&doc, div).as_deref(),
            Some("https://example.com/a.png"),
            "unhover should flush the restored background image"
        );
    }
}

#[cfg(test)]
mod font_face_override_tests {
    use super::*;
    use crate::net::{FontFaceOverrides, Resource, ResourceLoadResponse};

    /// Regression-pin for the `@font-face` descriptor-honouring fix.
    ///
    /// The bug was that `Resource::Font` carried only the raw font bytes,
    /// so `load_resource` registered fonts with `info_override = None` and
    /// parley fell back to the TTF's internal `name` table. After the fix,
    /// `Resource::Font` carries `FontFaceOverrides` and `load_resource`
    /// builds a `FontInfoOverride` from them — meaning a CSS-declared
    /// `font-family` alias wins over the file's own metadata.
    ///
    /// We drive `load_resource` directly with a fabricated response rather
    /// than go through HTML parsing → `fetch_font_face`, because the
    /// downstream HTML parser lives in `blitz-html` (would be a circular
    /// crate dependency). The mapping from `@font-face` descriptors into
    /// `FontFaceOverrides` is covered by the unit tests in `net.rs`; this
    /// test pins the load-side of the pipeline.
    #[test]
    fn font_face_overrides_alias_family_name() {
        const ALIAS: &str = "AliasedFamily";

        let mut document = BaseDocument::new(DocumentConfig::default());

        // Sanity: the alias name is not registered before we feed the font.
        {
            let mut ctx = document.font_ctx.lock().unwrap();
            assert!(
                ctx.collection.family_id(ALIAS).is_none(),
                "alias must not exist before registration",
            );
        }

        // Drive `load_resource` with a `Resource::Font` whose overrides
        // assert the CSS-side family name. We use the bullet font as a
        // valid font payload — its internal `name` table is irrelevant to
        // the assertion; what matters is whether the override wins.
        let response = ResourceLoadResponse {
            request_id: 0,
            node_id: None,
            resolved_url: Some(String::from("test://aliased-family")),
            result: Ok(Resource::Font(
                blitz_traits::net::Bytes::from_static(crate::BULLET_FONT),
                FontFaceOverrides {
                    family_name: Some(String::from(ALIAS)),
                    weight: Some(800.0),
                    style: Some(parley::fontique::FontStyle::Italic),
                },
            )),
        };
        document.load_resource(response);

        // The override must have taken effect: parley's `Collection` now
        // resolves the CSS-declared alias to a registered family.
        let mut ctx = document.font_ctx.lock().unwrap();
        let family_id = ctx
            .collection
            .family_id(ALIAS)
            .expect("CSS-declared family name should be registered as a family alias");
        let resolved_name = ctx
            .collection
            .family_name(family_id)
            .expect("family id should resolve back to a name");
        assert_eq!(
            resolved_name, ALIAS,
            "registered family should report the CSS-declared name, \
             not the font file's internal `name` table entry",
        );
    }
}
