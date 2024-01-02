use crate::{node::FlowType, Node};
use atomic_refcell::AtomicRefCell;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, RcDom};
use selectors::matching::QuirksMode;
use slab::Slab;
use std::{cell::RefCell, pin::Pin};
use style::{
    data::ElementData,
    dom::{TDocument, TNode},
    media_queries::{Device, MediaList},
    selector_parser::SnapshotMap,
    shared_lock::{SharedRwLock, StylesheetGuards},
    stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet},
    stylist::Stylist,
};
use taffy::{
    prelude::{AvailableSpace, Layout, Style},
    Cache,
};

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
}

impl Document {
    pub fn new(device: Device) -> Self {
        let quirks = QuirksMode::NoQuirks;
        let stylist = Stylist::new(device, quirks);
        let snapshots = SnapshotMap::new();
        let nodes = Box::new(Slab::new());
        let guard = SharedRwLock::new();
        Self {
            guard,
            nodes,
            stylist,
            snapshots,
        }
    }

    pub fn tree(&self) -> &Slab<Node> {
        &self.nodes
    }

    pub fn root_node(&self) -> &Node {
        &self.nodes[0]
    }

    pub fn root_element(&self) -> &Node {
        TDocument::as_node(&self.root_node())
            .first_child()
            .unwrap()
            .as_element()
            .unwrap()
    }

    /// Write some html to the buffer
    ///
    /// todo: this should be a stream implementing the htmlsink buffer thing
    ///
    /// For now we just convert the string to a dom tree and then walk it
    /// Eventually we want to build dom nodes from dioxus mutatiosn, however that's not exposed yet
    pub fn write(&mut self, content: String) {
        // parse the html into a document
        let document = html5ever::parse_document(RcDom::default(), Default::default())
            .from_utf8()
            .read_from(&mut content.as_bytes())
            .unwrap();

        fill_slab_with_handles(
            &mut self.nodes,
            document.document.clone(),
            0,
            None,
            &self.guard,
        );
    }

    pub fn add_stylesheet(&mut self, css: &str) {
        use style::servo_arc::Arc;

        let data = Stylesheet::from_str(
            css,
            servo_url::ServoUrl::from_url("data:text/css;charset=utf-8;base64,".parse().unwrap()),
            Origin::UserAgent,
            Arc::new(self.guard.wrap(MediaList::empty())),
            self.guard.clone(),
            None,
            None,
            QuirksMode::NoQuirks,
            0,
            AllowImportRules::Yes,
        );

        self.stylist
            .append_stylesheet(DocumentStyleSheet(Arc::new(data)), &self.guard.read());

        self.stylist
            .force_stylesheet_origins_dirty(Origin::Author.into());
    }

    /// Restyle the tree and then relayout it
    pub fn resolve(&mut self) {
        // we need to resolve stylist first since it will need to drive our layout bits
        self.resolve_stylist();

        // Merge stylo into taffy
        self.flush_styles_to_layout(vec![0]);

        // Next we resolve layout with the data resolved by stlist
        self.resolve_layout();
    }

    /// Update the device and reset the stylist to process the new size
    pub fn set_stylist_device(&mut self, device: Device) {
        let guard = &self.guard;
        let guards = StylesheetGuards {
            author: &guard.read(),
            ua_or_user: &guard.read(),
        };
        self.stylist.set_device(device, &guards);
    }

    /// Walk the nodes now that they're properly styled and transfer their styles to the taffy style system
    /// Ideally we could just break apart the styles into ECS bits, but alas
    ///
    /// Todo: update taffy to use an associated type instead of slab key
    /// Todo: update taffy to support traited styles so we don't even need to rely on taffy for storage
    pub fn resolve_layout(&mut self) {
        let size = self.stylist.device().au_viewport_size();

        let available_space = taffy::Size {
            width: AvailableSpace::Definite(size.width.to_f32_px() as _),
            height: AvailableSpace::Definite(size.height.to_f32_px() as _),
        };

        dbg!(available_space);

        let root = 0_usize;

        taffy::compute_root_layout(self, root.into(), available_space);
    }

    pub fn set_document(&mut self, content: String) {}

    pub fn add_element(&mut self) {}
}

// Assign IDs to the RcDom nodes by walking the tree and pushing them into the slab
// We just care that the root is 0, all else can be whatever
// Returns the node that just got inserted
fn fill_slab_with_handles(
    slab: &mut Slab<Node>,
    node: Handle,
    child_index: usize,
    parent: Option<usize>,
    guard: &SharedRwLock,
) -> usize {
    // todo: we want to skip filling comments/scripts/control, etc
    // Dioxus-rsx won't generate this however, so we're fine for now, but elements and text nodes are different

    // Reserve an entry
    let id = {
        let slab_ptr = slab as *mut Slab<Node>;
        let entry = slab.vacant_entry();
        let id = entry.key();
        let data: AtomicRefCell<ElementData> = Default::default();
        let style = Style::DEFAULT;
        entry.insert(Node {
            id,
            style,
            child_idx: child_index,
            children: vec![],
            node: node.clone(),
            parent,
            flow: FlowType::Block,
            cache: Cache::new(),
            // dom_data: todo!(),
            data,
            unrounded_layout: Layout::new(),
            final_layout: Layout::new(),
            tree: slab_ptr,
            guard: guard.clone(),
        });
        id
    };

    // Now go insert its children. We want their IDs to come back here so we know how to walk them.
    // We'll want some sort of linked list thing too to implement NextSibiling, etc
    // We're going to accumulate the children IDs here and then go back and edit the entry
    // All this dance is to make the borrow checker happy.
    slab[id].children = node
        .children
        .borrow()
        .iter()
        .enumerate()
        .map(|(idx, child)| fill_slab_with_handles(slab, child.clone(), idx, Some(id), guard))
        .collect();

    id
}
