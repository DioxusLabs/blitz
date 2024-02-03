use crate::{
    node::{DomData, FlowType},
    Node,
};
use atomic_refcell::AtomicRefCell;
use html5ever::tendril::{Tendril, TendrilSink};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use selectors::matching::QuirksMode;
use slab::Slab;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    fmt::Write,
    pin::Pin,
    rc::Rc,
};
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

    pub(crate) nodes_to_id: HashMap<String, usize>,
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
        servo_config::set_pref!(layout.flexbox.enabled, true);
        servo_config::set_pref!(layout.legacy_layout, true);
        servo_config::set_pref!(layout.columns.enabled, true);

        Self {
            guard,
            nodes,
            stylist,
            snapshots,
            nodes_to_id,
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

        self.populate_from_rc_dom(&[document.document.clone()], None);
    }

    pub fn populate_from_rc_dom(&mut self, children: &[Handle], parent: Option<usize>) {
        for (child_idx, node) in children.into_iter().enumerate() {
            // Create this node, absorbing any script/style data.
            let id = self.add_node(node, child_idx, parent);

            // Add this node to its parent's list of children.
            if let Some(parent) = parent {
                self.nodes[parent].children.push(id);
            }

            // Now go insert its children. We want their IDs to come back here so we know how to walk them.
            self.populate_from_rc_dom(&node.children.borrow(), Some(id));
        }
    }

    pub fn add_node(&mut self, node: &Handle, child_idx: usize, parent: Option<usize>) -> usize {
        let slab_ptr = self.nodes.as_mut() as *mut Slab<Node>;
        let entry = self.nodes.vacant_entry();
        let id = entry.key();
        let data: AtomicRefCell<ElementData> = Default::default();
        let style = Style::DEFAULT;

        let val = Node {
            id,
            style,
            child_idx,
            children: vec![],
            node: node.clone(),
            parent,
            flow: FlowType::Block,
            cache: Cache::new(),
            data,
            unrounded_layout: Layout::new(),
            final_layout: Layout::new(),
            tree: slab_ptr,
            guard: self.guard.clone(),
            additional_data: DomData::default(),
        };

        let entry = entry.insert(val);

        match &node.data {
            NodeData::Element {
                name,
                attrs,
                template_contents,
                ..
            } => {
                // If the node has an ID, store it in the ID map.
                if let Some(node_id) = attrs
                    .borrow()
                    .iter()
                    .find(|attr| attr.name.local.as_ref() == "id")
                {
                    self.nodes_to_id.insert(node_id.value.to_string(), id);
                }

                //
                match name.local.as_ref() {
                    // Attach the style to the document
                    "style" => {
                        let mut css = String::new();
                        for child in node.children.borrow().iter() {
                            match &child.data {
                                NodeData::Text { contents } => {
                                    css.push_str(&contents.borrow().to_string());
                                }
                                _ => {}
                            }
                        }
                        // unescape the css
                        let css = html_escape::decode_html_entities(&css);

                        self.add_stylesheet(&css);
                    }

                    // Create a shadow element and attach it to this node
                    "input" => {
                        // get the value and/or placeholder:
                        let mut value = None;
                        let mut placeholder = None;
                        for attr in attrs.borrow().iter() {
                            match attr.name.local.as_ref() {
                                "value" => {
                                    value = Some(attr.value.to_string());
                                }
                                "placeholder" => {
                                    placeholder = Some(attr.value.to_string());
                                }
                                _ => {}
                            }
                        }

                        if let Some(value) = value {
                            let mut tendril: Tendril<html5ever::tendril::fmt::UTF8> =
                                Tendril::new();

                            tendril.write_str(value.as_str()).unwrap();

                            let contents: RefCell<Tendril<html5ever::tendril::fmt::UTF8>> =
                                RefCell::new(tendril);

                            let handle = Handle::new(markup5ever_rcdom::Node {
                                parent: Cell::new(Some(Rc::downgrade(node))),
                                children: Default::default(),
                                data: NodeData::Text { contents },
                            });

                            // inserted as a child of the input
                            let shadow = self.add_node(&handle, 0, Some(id));

                            // attach it to its parent
                            self.nodes[id].children.push(shadow);
                        }
                    }

                    // todo: Load images
                    "img" => {}

                    // Todo: Load scripts
                    "script" => {}

                    // Load template elements (unpaired usually)
                    "template" => {
                        if let Some(template_contents) = template_contents.borrow().as_ref() {
                            let id = self
                                .populate_from_rc_dom(&template_contents.children.borrow(), None);
                        }
                    }

                    _ => entry.flush_style_attribute(),
                }
            }
            // markup5ever_rcdom::NodeData::Document => todo!(),
            // markup5ever_rcdom::NodeData::Doctype { name, public_id, system_id } => todo!(),
            // markup5ever_rcdom::NodeData::Text { contents } => todo!(),
            // markup5ever_rcdom::NodeData::Comment { contents } => todo!(),
            // markup5ever_rcdom::NodeData::ProcessingInstruction { target, contents } => todo!(),
            _ => {}
        }

        id
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
        self.flush_styles_to_layout(vec![self.root_element().id], None, taffy::Display::Block);

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
        let res = self
            .stylist
            .media_features_change_changed_style(&guards, &device);
        dbg!(res);
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
            // width: AvailableSpace::MaxContent,
            // height: AvailableSpace::Definite(10000000.0),
            // width: AvailableSpace::Definite(dbg!(1200.0)),
            // height: AvailableSpace::Definite(dbg!(2000.0)),
            // };
            width: AvailableSpace::Definite(dbg!(size.width.to_f32_px() as _)),
            height: AvailableSpace::Definite(dbg!((size.height.to_f32_px()) as _)),
            // height: AvailableSpace::Definite(1000000.0),
        };

        let root = 1_usize;

        taffy::compute_root_layout(self, root.into(), available_space);
    }

    pub fn set_document(&mut self, content: String) {}

    pub fn add_element(&mut self) {}
}
