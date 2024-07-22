use crate::events::RendererEvent;
use crate::node::TextBrush;
use crate::{Node, NodeData, TextNodeData};
// use quadtree_rs::Quadtree;
use selectors::{matching::QuirksMode, Element};
use slab::Slab;
use std::any::Any;
use std::collections::{HashMap, HashSet, VecDeque};
use style::invalidation::element::restyle_hints::RestyleHint;
use style::selector_parser::ServoElementSnapshot;
use style::servo::media_queries::FontMetricsProvider;
use style::servo_arc::Arc as ServoArc;
use style::values::specified::box_::DisplayOutside;
use style::{
    dom::{TDocument, TNode},
    media_queries::{Device, MediaList},
    selector_parser::SnapshotMap,
    shared_lock::{SharedRwLock, StylesheetGuards},
    stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet, UrlExtraData},
    stylist::Stylist,
};
use style_traits::dom::ElementState;
use taffy::AvailableSpace;
use url::Url;

// TODO: implement a proper font metrics provider
#[derive(Debug, Clone)]
pub struct DummyFontMetricsProvider;
impl FontMetricsProvider for DummyFontMetricsProvider {
    fn query_font_metrics(
        &self,
        _vertical: bool,
        _font: &style::properties::style_structs::Font,
        _base_size: style::values::computed::CSSPixelLength,
        _in_media_query: bool,
        _retrieve_math_scales: bool,
    ) -> style::font_metrics::FontMetrics {
        Default::default()
    }
}

pub trait DocumentLike: AsRef<Document> + AsMut<Document> + Into<Document> + 'static {
    fn poll(&mut self, _cx: std::task::Context) -> bool {
        // Default implementation does nothing
        false
    }

    fn handle_event(&mut self, _event: RendererEvent) -> bool {
        // Default implementation does nothing
        false
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl DocumentLike for Document {}

pub struct Document {
    /// A bump-backed tree
    ///
    /// Both taffy and stylo traits are implemented for this.
    /// We pin the tree to a guarantee to the nodes it creates that the tree is stable in memory.
    ///
    /// There is no way to create the tree - publicly or privately - that would invalidate that invariant.
    pub(crate) nodes: Box<Slab<Node>>,

    pub(crate) guard: SharedRwLock,

    /// The styling engine of firefox
    pub(crate) stylist: Stylist,

    // caching for the stylist
    pub(crate) snapshots: SnapshotMap,

    pub(crate) nodes_to_id: HashMap<String, usize>,

    /// Base url for resolving linked resources (stylesheets, images, fonts, etc)
    pub(crate) base_url: Option<url::Url>,

    /// The quadtree we use for hit-testing
    // pub(crate) quadtree: Quadtree<u64, usize>,

    // The HiDPI display scale
    pub(crate) scale: f32,

    pub(crate) stylesheets: HashMap<String, DocumentStyleSheet>,

    /// A Parley font context
    pub(crate) font_ctx: parley::FontContext,
    /// A Parley layout context
    pub(crate) layout_ctx: parley::LayoutContext<TextBrush>,

    pub(crate) hover_node_id: Option<usize>,

    pub changed: HashSet<usize>,
}

impl Document {
    pub fn new(device: Device) -> Self {
        let quirks = QuirksMode::NoQuirks;
        let stylist = Stylist::new(device, quirks);
        let snapshots = SnapshotMap::new();
        let nodes = Box::new(Slab::new());
        let guard = SharedRwLock::new();
        let nodes_to_id = HashMap::new();

        // Make sure we turn on servo features
        style_config::set_bool("layout.flexbox.enabled", true);
        style_config::set_bool("layout.grid.enabled", true);
        style_config::set_bool("layout.legacy_layout", true);
        style_config::set_bool("layout.columns.enabled", true);

        let mut doc = Self {
            guard,
            nodes,
            stylist,
            snapshots,
            nodes_to_id,
            scale: 1.0,
            base_url: None,
            // quadtree: Quadtree::new(20),
            stylesheets: HashMap::new(),
            font_ctx: parley::FontContext::default(),
            layout_ctx: parley::LayoutContext::new(),

            hover_node_id: None,
            changed: HashSet::new(),
        };

        // Initialise document with root Document node
        doc.create_node(NodeData::Document);

        doc
    }

    /// Set base url for resolving linked resources (stylesheets, images, fonts, etc)
    pub fn set_base_url(&mut self, url: &str) {
        self.base_url = Url::parse(url).ok();
    }

    pub fn guard(&self) -> &SharedRwLock {
        &self.guard
    }

    pub fn tree(&self) -> &Slab<Node> {
        &self.nodes
    }

    pub fn get_node(&self, node_id: usize) -> Option<&Node> {
        self.nodes.get(node_id)
    }

    pub fn get_node_mut(&mut self, node_id: usize) -> Option<&mut Node> {
        self.nodes.get_mut(node_id)
    }

    pub fn root_node(&self) -> &Node {
        &self.nodes[0]
    }

    pub fn root_element(&self) -> &Node {
        TDocument::as_node(&self.root_node())
            .first_element_child()
            .unwrap()
            .as_element()
            .unwrap()
    }

    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale;
    }

    pub fn create_node(&mut self, node_data: NodeData) -> usize {
        let slab_ptr = self.nodes.as_mut() as *mut Slab<Node>;
        let entry = self.nodes.vacant_entry();
        let id = entry.key();
        let guard = self.guard.clone();

        entry.insert(Node::new(slab_ptr, id, guard, node_data));

        // self.quadtree.insert(
        //     AreaBuilder::default()
        //         .anchor(quadtree_rs::point::Point { x: 4, y: 5 })
        //         .dimensions((2, 3))
        //         .build()
        //         .unwrap(),
        //     id as usize,
        // );

        // Mark the new node as changed.
        self.changed.insert(id);

        id
    }

    pub fn create_text_node(&mut self, text: &str) -> usize {
        let content = text.to_string();
        let data = NodeData::Text(TextNodeData::new(content));
        self.create_node(data)
    }

    pub fn deep_clone_node(&mut self, node_id: usize) -> usize {
        // Load existing node
        let node = &self.nodes[node_id];
        let data = node.raw_dom_data.clone();
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

    pub fn insert_before(&mut self, node_id: usize, inserted_node_ids: &[usize]) {
        // let count = inserted_node_ids.len();

        // self.print_tree();

        let node = &self.nodes[node_id];
        let node_child_idx = node.child_idx;

        let parent_id = node.parent.unwrap();
        let parent = &mut self.nodes[parent_id];

        // Mark the node's parent as changed.
        self.changed.insert(parent_id);

        let mut children = std::mem::take(&mut parent.children);
        children.splice(
            node_child_idx..node_child_idx,
            inserted_node_ids.iter().copied(),
        );

        // Update child_idx and parent values
        let mut child_idx = node_child_idx;
        while child_idx < children.len() {
            let child_id = children[child_idx];
            let node = &mut self.nodes[child_id];
            node.child_idx = child_idx;
            node.parent = Some(parent_id);
            child_idx += 1;
        }

        self.nodes[parent_id].children = children;
    }

    pub fn append(&mut self, node_id: usize, appended_node_ids: &[usize]) {
        let node = &self.nodes[node_id];
        // let node_child_idx = node.child_idx;
        let parent_id = node.parent.unwrap();
        let parent = &mut self.nodes[parent_id];

        let mut children = std::mem::take(&mut parent.children);
        let old_len = children.len();
        children.extend_from_slice(appended_node_ids);

        // Update child_idx and parent values
        let mut child_idx = old_len;
        while child_idx < children.len() {
            let child_id = children[child_idx];
            let node = &mut self.nodes[child_id];
            node.child_idx = child_idx;
            node.parent = Some(parent_id);
            child_idx += 1;
        }

        self.nodes[parent_id].children = children;
    }

    pub fn remove_node(&mut self, node_id: usize) -> Option<Node> {
        fn remove_node_ignoring_parent(doc: &mut Document, node_id: usize) -> Option<Node> {
            let node = doc.nodes.try_remove(node_id);
            if let Some(node) = &node {
                for &child in &node.children {
                    remove_node_ignoring_parent(doc, child);
                }
            }
            node
        }

        let node = remove_node_ignoring_parent(self, node_id);

        // Update child_idx values
        if let Some(Node {
            mut child_idx,
            parent: Some(parent_id),
            ..
        }) = node
        {
            let parent = &mut self.nodes[parent_id];

            let mut children = std::mem::take(&mut parent.children);
            children.remove(child_idx);

            // Update child_idx and parent values
            while child_idx < children.len() {
                let child_id = children[child_idx];
                let node = &mut self.nodes[child_id];
                node.child_idx = child_idx;
                child_idx += 1;
            }

            self.nodes[parent_id].children = children;
        }

        node
    }

    pub fn resolve_url(&self, raw: &str) -> url::Url {
        match &self.base_url {
            Some(base_url) => base_url.join(raw).unwrap(),
            None => url::Url::parse(raw).unwrap(),
        }
    }

    pub fn flush_child_indexes(&mut self, target_id: usize, child_idx: usize, _level: usize) {
        let node = &mut self.nodes[target_id];
        node.child_idx = child_idx;

        // println!("{} {} {:?} {:?}", "  ".repeat(level), target_id, node.parent, node.children);

        for (i, child_id) in node.children.clone().iter().enumerate() {
            self.flush_child_indexes(*child_id, i, _level + 1)
        }
    }

    pub fn print_tree(&self) {
        crate::util::walk_tree(0, self.root_node());
    }

    pub fn process_style_element(&mut self, target_id: usize) {
        let css = self.nodes[target_id].text_content();
        let css = html_escape::decode_html_entities(&css);
        self.add_stylesheet(&css);
    }

    pub fn remove_stylehsheet(&mut self, contents: &str) {
        if let Some(sheet) = self.stylesheets.remove(contents) {
            self.stylist.remove_stylesheet(sheet, &self.guard.read());
        }
    }

    pub fn add_stylesheet(&mut self, css: &str) {
        let data = Stylesheet::from_str(
            css,
            UrlExtraData::from(
                "data:text/css;charset=utf-8;base64,"
                    .parse::<Url>()
                    .unwrap(),
            ),
            Origin::UserAgent,
            ServoArc::new(self.guard.wrap(MediaList::empty())),
            self.guard.clone(),
            None,
            None,
            QuirksMode::NoQuirks,
            AllowImportRules::Yes,
        );

        let sheet = DocumentStyleSheet(ServoArc::new(data));

        self.stylesheets.insert(css.to_string(), sheet.clone());

        self.stylist.append_stylesheet(sheet, &self.guard.read());

        self.stylist
            .force_stylesheet_origins_dirty(Origin::Author.into());
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
            self.snapshots.insert(
                opaque_node_id,
                ServoElementSnapshot {
                    state: Some(node.element_state),
                    attrs: None,
                    changed_attrs: Vec::new(),
                    class_changed: false,
                    id_changed: false,
                    other_attributes_changed: false,
                },
            );
        }
    }

    /// Restyle the tree and then relayout it
    pub fn resolve(&mut self) {
        if TDocument::as_node(&&self.nodes[0])
            .first_element_child()
            .is_none()
        {
            println!("No DOM - not resolving");
            return;
        }

        // we need to resolve stylist first since it will need to drive our layout bits
        self.resolve_stylist();

        // Fix up tree for layout (insert anonymous blocks as necessary, etc)
        self.resolve_layout_children();

        // Merge stylo into taffy
        self.flush_styles_to_layout(vec![self.root_element().id]);

        // Next we resolve layout with the data resolved by stlist
        self.resolve_layout();
    }

    // Takes (x, y) co-ordinates (relative to the )
    pub fn hit(&self, x: f32, y: f32) -> Option<usize> {
        if TDocument::as_node(&&self.nodes[0])
            .first_element_child()
            .is_none()
        {
            println!("No DOM - not resolving");
            return None;
        }

        self.root_element().hit(x, y)
    }

    pub fn set_hover_to(&mut self, x: f32, y: f32) -> bool {
        let hover_node_id = self.hit(x, y);

        if hover_node_id != self.hover_node_id {
            if let Some(new_hover_id) = hover_node_id {
                self.reset_styles(new_hover_id);
            }
            if let Some(old_hover_id) = self.hover_node_id {
                self.reset_styles(old_hover_id);
            }
            let mut maybe_id = self.hover_node_id;
            while let Some(id) = maybe_id {
                self.nodes[id].is_hovered = false;
                self.nodes[id].element_state.remove(ElementState::HOVER);
                if let Some(element_data) = self.nodes[id].stylo_element_data.borrow_mut().as_mut()
                {
                    element_data.hint.insert(RestyleHint::RESTYLE_SELF);
                }

                self.snapshot_node(id);

                maybe_id = self.nodes[id].parent;
            }

            let mut maybe_id = hover_node_id;
            while let Some(id) = maybe_id {
                self.nodes[id].is_hovered = true;
                self.nodes[id].element_state.insert(ElementState::HOVER);
                if let Some(element_data) = self.nodes[id].stylo_element_data.borrow_mut().as_mut()
                {
                    element_data.hint.insert(RestyleHint::RESTYLE_SELF);
                }

                self.snapshot_node(id);

                maybe_id = self.nodes[id].parent;
            }

            self.hover_node_id = hover_node_id;

            true
        } else {
            false
        }
    }

    pub fn get_hover_node_id(&self) -> Option<usize> {
        self.hover_node_id
    }

    /// Update the device and reset the stylist to process the new size
    pub fn set_stylist_device(&mut self, device: Device) {
        let guard = &self.guard;
        let guards = StylesheetGuards {
            author: &guard.read(),
            ua_or_user: &guard.read(),
        };
        let origins = self.stylist.set_device(device, &guards);
        self.stylist.force_stylesheet_origins_dirty(origins);
    }
    pub fn stylist_device(&mut self) -> &Device {
        self.stylist.device()
    }

    pub fn invalidate_inline_layout(&mut self, node_id: usize) {
        let node = self.get_node_mut(node_id).unwrap();
        let style = node.display_style();
        let NodeData::Element(ref mut element) = node.raw_dom_data else {
            return;
        };
        element.inline_layout = None;
        // TODO: When let-chains lend in stable, rewrite to be nicer
        let Some(style) = style else {
            return;
        };
        let Some(parent) = node.parent else {
            return;
        };
        if style.outside() == DisplayOutside::Inline {
            self.invalidate_inline_layout(parent);
        }
    }

    // TODO: When class, style, or hover changes, we need to clean up all the unchanged stuff in cache.
    // For now, we clean up only inline layout as we don't cache other style related stuff.
    pub fn reset_styles(&mut self, element_id: usize) {
        self.invalidate_inline_layout(element_id);
        let node = self.get_node(element_id).unwrap();
        // I hate this clone
        for child_id in node.children.clone().iter().copied() {
            self.reset_styles(child_id);
        }
    }

    /// Ensure that the layout_children field is populated for all nodes
    pub fn resolve_layout_children(&mut self) {
        let root_node_id = self.root_node().id;
        resolve_layout_children_recursive(self, root_node_id);

        pub fn resolve_layout_children_recursive(doc: &mut Document, node_id: usize) {
            doc.ensure_layout_children(node_id);

            let children = std::mem::take(&mut doc.nodes[node_id].children);

            for child_id in children.iter().copied() {
                resolve_layout_children_recursive(doc, child_id);
            }

            doc.nodes[node_id].children = children;
        }
    }

    /// Walk the nodes now that they're properly styled and transfer their styles to the taffy style system
    /// Ideally we could just break apart the styles into ECS bits, but alas
    ///
    /// Todo: update taffy to use an associated type instead of slab key
    /// Todo: update taffy to support traited styles so we don't even need to rely on taffy for storage
    pub fn resolve_layout(&mut self) {
        let size = self.stylist.device().au_viewport_size();

        let available_space = taffy::Size {
            width: AvailableSpace::Definite(size.width.to_f32_px()),
            height: AvailableSpace::Definite(size.height.to_f32_px()),
        };

        let root_node_id = taffy::NodeId::from(self.root_element().id);

        // println!("\n\nRESOLVE LAYOUT\n===========\n");

        taffy::compute_root_layout(self, root_node_id, available_space);
        taffy::round_layout(self, root_node_id);

        // println!("\n\n");
        // taffy::print_tree(self, root_node_id)
    }

    pub fn set_document(&mut self, _content: String) {}

    pub fn add_element(&mut self) {}

    pub fn visit<F>(&self, mut visit: F)
    where
        F: FnMut(usize, &Node),
    {
        let mut stack = VecDeque::new();
        stack.push_front(0);

        while let Some(node_key) = stack.pop_back() {
            let node = &self.nodes[node_key];
            visit(node_key, node);

            for &child_key in &node.children {
                stack.push_front(child_key);
            }
        }
    }
}

impl AsRef<Document> for Document {
    fn as_ref(&self) -> &Document {
        self
    }
}

impl AsMut<Document> for Document {
    fn as_mut(&mut self) -> &mut Document {
        self
    }
}
