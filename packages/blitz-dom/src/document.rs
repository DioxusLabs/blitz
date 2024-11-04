use crate::events::{EventData, HitResult, RendererEvent};
use crate::node::{ImageData, NodeSpecificData, TextBrush};
use crate::{ElementNodeData, Node, NodeData, TextNodeData, Viewport};
use app_units::Au;
use html5ever::local_name;
use parley::PlainEditorOp;
use peniko::kurbo;
use string_cache::Atom;
use style::attr::{AttrIdentifier, AttrValue};
use style::values::computed::Overflow;
use style::values::GenericAtomIdent;
use winit::keyboard::{Key, NamedKey};
// use quadtree_rs::Quadtree;
use crate::net::Resource;
use selectors::{matching::QuirksMode, Element};
use slab::Slab;
use std::any::Any;
use std::collections::{BTreeMap, Bound, HashMap, HashSet, VecDeque};
use style::selector_parser::ServoElementSnapshot;
use style::servo::media_queries::FontMetricsProvider;
use style::servo_arc::Arc as ServoArc;
use style::values::computed::ui::CursorKind;
use style::{
    dom::{TDocument, TNode},
    media_queries::{Device, MediaList},
    selector_parser::SnapshotMap,
    shared_lock::{SharedRwLock, StylesheetGuards},
    stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet, UrlExtraData},
    stylist::Stylist,
};
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

    fn base_size_for_generic(
        &self,
        generic: style::values::computed::font::GenericFontFamily,
    ) -> style::values::computed::Length {
        let size = match generic {
            style::values::computed::font::GenericFontFamily::Monospace => 13.0,
            _ => 16.0,
        };
        style::values::computed::Length::from(Au::from_f32_px(size))
    }
}

pub trait DocumentLike: AsRef<Document> + AsMut<Document> + Into<Document> + 'static {
    fn poll(&mut self, _cx: std::task::Context) -> bool {
        // Default implementation does nothing
        false
    }

    fn handle_event(&mut self, _event: RendererEvent) {
        // Default implementation does nothing
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub struct Document {
    /// A bump-backed tree
    ///
    /// Both taffy and stylo traits are implemented for this.
    /// We pin the tree to a guarantee to the nodes it creates that the tree is stable in memory.
    ///
    /// There is no way to create the tree - publicly or privately - that would invalidate that invariant.
    pub nodes: Box<Slab<Node>>,

    pub(crate) guard: SharedRwLock,

    /// The styling engine of firefox
    pub(crate) stylist: Stylist,

    // caching for the stylist
    pub(crate) snapshots: SnapshotMap,

    pub(crate) nodes_to_id: HashMap<String, usize>,

    /// Base url for resolving linked resources (stylesheets, images, fonts, etc)
    pub(crate) base_url: Option<url::Url>,

    // /// The quadtree we use for hit-testing
    // pub(crate) quadtree: Quadtree<u64, usize>,

    // Viewport details such as the dimensions, HiDPI scale, and zoom factor,
    pub(crate) viewport: Viewport,

    // Scroll within our viewport
    pub(crate) viewport_scroll: kurbo::Point,

    /// Stylesheets added by the useragent
    /// where the key is the hashed CSS
    pub(crate) ua_stylesheets: HashMap<String, DocumentStyleSheet>,

    pub(crate) nodes_to_stylesheet: BTreeMap<usize, DocumentStyleSheet>,

    /// A Parley font context
    pub font_ctx: parley::FontContext,

    /// A Parley layout context
    pub layout_ctx: parley::LayoutContext<TextBrush>,

    /// The node which is currently hovered (if any)
    pub(crate) hover_node_id: Option<usize>,
    /// The node which is currently focussed (if any)
    pub(crate) focus_node_id: Option<usize>,

    pub changed: HashSet<usize>,
}

impl DocumentLike for Document {
    fn handle_event(&mut self, event: RendererEvent) {
        let target_node_id = event.target;

        match event.data {
            EventData::Click { x, y, .. } => {
                let hit = self.hit(x, y);
                if let Some(hit) = hit {
                    assert!(hit.node_id == event.target);

                    let node = &mut self.nodes[hit.node_id];
                    let content_box_offset = taffy::Point {
                        x: node.final_layout.padding.left + node.final_layout.border.left,
                        y: node.final_layout.padding.top + node.final_layout.border.top,
                    };
                    let Some(el) = node.raw_dom_data.downcast_element_mut() else {
                        return;
                    };

                    let disabled = el.attr(local_name!("disabled")).is_some();
                    if disabled {
                        return;
                    }

                    if let NodeSpecificData::TextInput(ref mut text_input_data) =
                        el.node_specific_data
                    {
                        let x = (hit.x - content_box_offset.x) as f64 * self.viewport.scale_f64();
                        let y = (hit.y - content_box_offset.y) as f64 * self.viewport.scale_f64();
                        text_input_data.editor.transact(
                            &mut self.font_ctx,
                            &mut self.layout_ctx,
                            [PlainEditorOp::MoveToPoint(x as f32, y as f32)],
                        );

                        self.set_focus_to(hit.node_id);
                    } else if el.name.local == local_name!("input")
                        && matches!(el.attr(local_name!("type")), Some("checkbox"))
                    {
                        Document::toggle_checkbox(el);
                        self.set_focus_to(hit.node_id);
                    }
                    // Clicking labels triggers click, and possibly input event, of associated input
                    else if el.name.local == local_name!("label") {
                        let node_id = node.id;
                        if let Some(target_node_id) = self
                            .label_bound_input_elements(node_id)
                            .first()
                            .map(|n| n.id)
                        {
                            let target_node = self.get_node_mut(target_node_id).unwrap();
                            if let Some(target_element) = target_node.element_data_mut() {
                                Document::toggle_checkbox(target_element);
                            }
                            self.set_focus_to(node_id);
                        }
                    }
                }
            }
            EventData::KeyPress { event, mods } => {
                if let Some(node_id) = self.focus_node_id {
                    if target_node_id != node_id {
                        return;
                    }

                    if !event.state.is_pressed() {
                        return;
                    }
                    #[allow(unused)]
                    let shift = mods.state().shift_key();
                    #[allow(unused)]
                    let action_mod = {
                        if cfg!(target_os = "macos") {
                            mods.state().super_key()
                        } else {
                            mods.state().control_key()
                        }
                    };

                    let node = &mut self.nodes[node_id];
                    let text_input_data = node
                        .raw_dom_data
                        .downcast_element_mut()
                        .and_then(|el| el.text_input_data_mut());

                    if let Some(input_data) = text_input_data {
                        macro_rules! transact {
                            ($op:expr) => {{
                                input_data.editor.transact(
                                    &mut self.font_ctx,
                                    &mut self.layout_ctx,
                                    [$op],
                                );
                            }};
                        }

                        match event.logical_key {
                            #[cfg(not(target_os = "android"))]
                            Key::Character(c)
                                if action_mod && matches!(c.as_str(), "c" | "x" | "v") =>
                            {
                                use clipboard_rs::{Clipboard, ClipboardContext};
                                use parley::layout::editor::ActiveText;

                                match c.to_lowercase().as_str() {
                                    "c" => {
                                        if let ActiveText::Selection(text) =
                                            input_data.editor.active_text()
                                        {
                                            let cb = ClipboardContext::new().unwrap();
                                            cb.set_text(text.to_owned()).ok();
                                        }
                                    }
                                    "x" => {
                                        if let ActiveText::Selection(text) =
                                            input_data.editor.active_text()
                                        {
                                            let cb = ClipboardContext::new().unwrap();
                                            cb.set_text(text.to_owned()).ok();
                                            transact!(PlainEditorOp::DeleteSelection)
                                        }
                                    }
                                    "v" => {
                                        let cb = ClipboardContext::new().unwrap();
                                        let text = cb.get_text().unwrap_or_default();
                                        transact!(PlainEditorOp::InsertOrReplaceSelection(
                                            text.into(),
                                        ))
                                    }
                                    _ => unreachable!(),
                                }
                            }
                            Key::Character(c)
                                if action_mod && matches!(c.to_lowercase().as_str(), "a") =>
                            {
                                if shift {
                                    transact!(PlainEditorOp::CollapseSelection)
                                } else {
                                    transact!(PlainEditorOp::SelectAll)
                                }
                            }
                            Key::Named(NamedKey::ArrowLeft) => {
                                if action_mod {
                                    if shift {
                                        transact!(PlainEditorOp::SelectWordLeft)
                                    } else {
                                        transact!(PlainEditorOp::MoveWordLeft)
                                    }
                                } else if shift {
                                    transact!(PlainEditorOp::SelectLeft)
                                } else {
                                    transact!(PlainEditorOp::MoveLeft)
                                }
                            }
                            Key::Named(NamedKey::ArrowRight) => {
                                if action_mod {
                                    if shift {
                                        transact!(PlainEditorOp::SelectWordRight)
                                    } else {
                                        transact!(PlainEditorOp::MoveWordRight)
                                    }
                                } else if shift {
                                    transact!(PlainEditorOp::SelectRight)
                                } else {
                                    transact!(PlainEditorOp::MoveRight)
                                }
                            }
                            Key::Named(NamedKey::ArrowUp) => {
                                if shift {
                                    transact!(PlainEditorOp::SelectUp)
                                } else {
                                    transact!(PlainEditorOp::MoveUp)
                                }
                            }
                            Key::Named(NamedKey::ArrowDown) => {
                                if shift {
                                    transact!(PlainEditorOp::SelectDown)
                                } else {
                                    transact!(PlainEditorOp::MoveDown)
                                }
                            }
                            Key::Named(NamedKey::Home) => {
                                if action_mod {
                                    if shift {
                                        transact!(PlainEditorOp::SelectToTextStart)
                                    } else {
                                        transact!(PlainEditorOp::MoveToTextStart)
                                    }
                                } else if shift {
                                    transact!(PlainEditorOp::SelectToLineStart)
                                } else {
                                    transact!(PlainEditorOp::MoveToLineStart)
                                }
                            }
                            Key::Named(NamedKey::End) => {
                                if action_mod {
                                    if shift {
                                        transact!(PlainEditorOp::SelectToTextEnd)
                                    } else {
                                        transact!(PlainEditorOp::MoveToTextEnd)
                                    }
                                } else if shift {
                                    transact!(PlainEditorOp::SelectToLineEnd)
                                } else {
                                    transact!(PlainEditorOp::MoveToLineEnd)
                                }
                            }
                            Key::Named(NamedKey::Delete) => {
                                if action_mod {
                                    transact!(PlainEditorOp::DeleteWord)
                                } else {
                                    transact!(PlainEditorOp::Delete)
                                }
                            }
                            Key::Named(NamedKey::Backspace) => {
                                if action_mod {
                                    transact!(PlainEditorOp::BackdeleteWord)
                                } else {
                                    transact!(PlainEditorOp::Backdelete)
                                }
                            }
                            Key::Named(NamedKey::Enter) => {
                                transact!(PlainEditorOp::InsertOrReplaceSelection("\n".into()))
                            }
                            Key::Named(NamedKey::Space) => {
                                transact!(PlainEditorOp::InsertOrReplaceSelection(" ".into()))
                            }
                            Key::Character(s) => {
                                transact!(PlainEditorOp::InsertOrReplaceSelection(s.into()))
                            }
                            _ => {}
                        };
                        println!("Sent text event to {}", node_id);
                    }
                }
            }
            EventData::Ime(_ime_event) => {
                // FIXME: Implement IME events on top of PlainEditor
                // if let Some(node_id) = self.focus_node_id {
                //     let node = &mut self.nodes[node_id];
                //     let text_input_data = node
                //         .raw_dom_data
                //         .downcast_element_mut()
                //         .and_then(|el| el.text_input_data_mut());
                //     if let Some(input_data) = text_input_data {
                //         let text_event = TextEvent::Ime(ime_event);
                //         input_data.editor.text_event(&text_event);
                //         println!("Sent ime event to {}", node_id);
                //     }
                // }
            }
            EventData::Hover => {}
        }
    }
}

impl Document {
    pub fn new(viewport: Viewport) -> Self {
        let device = viewport.make_device();
        let stylist = Stylist::new(device, QuirksMode::NoQuirks);
        let snapshots = SnapshotMap::new();
        let nodes = Box::new(Slab::new());
        let guard = SharedRwLock::new();
        let nodes_to_id = HashMap::new();

        // Make sure we turn on stylo features
        style_config::set_bool("layout.flexbox.enabled", true);
        style_config::set_bool("layout.grid.enabled", true);
        style_config::set_bool("layout.legacy_layout", true);
        style_config::set_bool("layout.columns.enabled", true);

        let font_ctx = Self::create_font_context();

        let mut doc = Self {
            guard,
            nodes,
            stylist,
            snapshots,
            nodes_to_id,
            viewport,
            viewport_scroll: kurbo::Point::ZERO,
            base_url: None,
            // quadtree: Quadtree::new(20),
            ua_stylesheets: HashMap::new(),
            nodes_to_stylesheet: BTreeMap::new(),
            font_ctx,
            layout_ctx: parley::LayoutContext::new(),

            hover_node_id: None,
            focus_node_id: None,
            changed: HashSet::new(),
        };

        // Initialise document with root Document node
        doc.create_node(NodeData::Document);

        doc
    }

    fn create_font_context() -> parley::FontContext {
        let mut ctx = parley::FontContext::default();
        ctx.collection
            .register_fonts(include_bytes!("moz-bullet-font.otf").to_vec());
        ctx
    }

    /// Set base url for resolving linked resources (stylesheets, images, fonts, etc)
    pub fn set_base_url(&mut self, url: &str) {
        self.base_url = Some(Url::parse(url).unwrap());
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

    pub fn get_focussed_node_id(&self) -> Option<usize> {
        self.focus_node_id
            .or(self.try_root_element().map(|el| el.id))
    }

    /// Find the label's bound input elements:
    /// the element id referenced by the "for" attribute of a given label element
    /// or the first input element which is nested in the label
    /// Note that although there should only be one bound element,
    /// we return all possibilities instead of just the first
    /// in order to allow the caller to decide which one is correct
    pub fn label_bound_input_elements(&self, label_node_id: usize) -> Vec<&Node> {
        let label_node = self.get_node(label_node_id).unwrap();
        let label_element = label_node.element_data().unwrap();
        if let Some(target_element_dom_id) = label_element.attr(local_name!("for")) {
            self.tree()
                .into_iter()
                .filter_map(|(_id, node)| {
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
                .collect()
        } else {
            label_node
                .children
                .iter()
                .filter_map(|child_id| {
                    let node = self.get_node(*child_id)?;
                    let element_data = node.element_data()?;
                    if element_data.name.local == local_name!("input") {
                        Some(node)
                    } else {
                        None
                    }
                })
                .collect()
        }
    }

    pub fn toggle_checkbox(el: &mut ElementNodeData) {
        let Some(is_checked) = el.checkbox_input_checked_mut() else {
            return;
        };
        *is_checked = !*is_checked;
    }

    pub fn root_node(&self) -> &Node {
        &self.nodes[0]
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

        let parent_id = node.parent.unwrap();
        let parent = &mut self.nodes[parent_id];
        let node_child_idx = parent
            .children
            .iter()
            .position(|id| *id == node_id)
            .unwrap();

        // Mark the node's parent as changed.
        self.changed.insert(parent_id);

        let mut children = std::mem::take(&mut parent.children);
        children.splice(
            node_child_idx..node_child_idx,
            inserted_node_ids.iter().copied(),
        );

        // Update parent values
        let mut child_idx = node_child_idx;
        while child_idx < children.len() {
            let child_id = children[child_idx];
            let node = &mut self.nodes[child_id];
            node.parent = Some(parent_id);
            child_idx += 1;
        }

        self.nodes[parent_id].children = children;
    }

    pub fn append(&mut self, node_id: usize, appended_node_ids: &[usize]) {
        let node = &self.nodes[node_id];
        let parent_id = node.parent.unwrap();
        self.nodes[parent_id]
            .children
            .extend_from_slice(appended_node_ids);

        // Update parent values
        for &child_id in appended_node_ids {
            self.nodes[child_id].parent = Some(parent_id);
        }
    }

    /// Remove the node from it's parent but don't drop it
    pub fn remove_node(&mut self, node_id: usize) {
        let node = &mut self.nodes[node_id];

        // Update child_idx values
        if let Some(parent_id) = node.parent.take() {
            let parent = &mut self.nodes[parent_id];
            parent.children.retain(|id| *id != node_id);
        }
    }

    pub fn remove_and_drop_node(&mut self, node_id: usize) -> Option<Node> {
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
        if let Some(parent_id) = node.as_ref().and_then(|node| node.parent) {
            let parent = &mut self.nodes[parent_id];
            parent.children.retain(|id| *id != node_id);
        }

        node
    }

    pub fn resolve_url(&self, raw: &str) -> url::Url {
        match &self.base_url {
            Some(base_url) => base_url.join(raw).unwrap(),
            None => url::Url::parse(raw).unwrap(),
        }
    }

    pub fn print_tree(&self) {
        crate::util::walk_tree(0, self.root_node());
    }

    pub fn print_subtree(&self, node_id: usize) {
        crate::util::walk_tree(0, &self.nodes[node_id]);
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
            UrlExtraData::from(
                "data:text/css;charset=utf-8;base64,"
                    .parse::<Url>()
                    .unwrap(),
            ),
            origin,
            ServoArc::new(self.guard.wrap(MediaList::empty())),
            self.guard.clone(),
            None,
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

    pub fn load_resource(&mut self, resource: Resource) {
        match resource {
            Resource::Css(node_id, css) => {
                self.add_stylesheet_for_node(css, node_id);
            }
            Resource::Image(node_id, image) => {
                let node = self.get_node_mut(node_id).unwrap();
                node.element_data_mut().unwrap().node_specific_data =
                    NodeSpecificData::Image(ImageData::new(image))
            }
            Resource::Svg(node_id, tree) => {
                let node = self.get_node_mut(node_id).unwrap();
                node.element_data_mut().unwrap().node_specific_data = NodeSpecificData::Svg(*tree)
            }
            Resource::Font(bytes) => {
                self.font_ctx.collection.register_fonts(bytes.to_vec());
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
        self.flush_styles_to_layout(self.root_element().id);

        // Next we resolve layout with the data resolved by stlist
        self.resolve_layout();
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

    pub fn iter_children_mut(&mut self, node_id: usize, mut cb: impl FnMut(usize, &mut Document)) {
        let children = std::mem::take(&mut self.nodes[node_id].children);
        for child_id in children.iter().cloned() {
            cb(child_id, self);
        }
        self.nodes[node_id].children = children;
    }

    pub fn iter_subtree_mut(&mut self, node_id: usize, mut cb: impl FnMut(usize, &mut Document)) {
        iter_subtree_mut_inner(self, node_id, &mut cb);
        fn iter_subtree_mut_inner(
            doc: &mut Document,
            node_id: usize,
            cb: &mut impl FnMut(usize, &mut Document),
        ) {
            let children = std::mem::take(&mut doc.nodes[node_id].children);
            for child_id in children.iter().cloned() {
                cb(child_id, doc);
                iter_subtree_mut_inner(doc, child_id, cb);
            }
            doc.nodes[node_id].children = children;
        }
    }

    pub fn iter_children_and_pseudos_mut(
        &mut self,
        node_id: usize,
        mut cb: impl FnMut(usize, &mut Document),
    ) {
        let before = self.nodes[node_id].before.take();
        if let Some(before_node_id) = before {
            cb(before_node_id, self)
        }
        self.nodes[node_id].before = before;

        self.iter_children_mut(node_id, &mut cb);

        let after = self.nodes[node_id].after.take();
        if let Some(after_node_id) = after {
            cb(after_node_id, self)
        }
        self.nodes[node_id].after = after;
    }

    pub fn next_node(&self, start: &Node, mut filter: impl FnMut(&Node) -> bool) -> Option<usize> {
        let start_id = start.id;
        let mut node = start;
        let mut look_in_children = true;
        loop {
            // Next is first child
            let next = if look_in_children && !node.children.is_empty() {
                let node_id = node.children[0];
                &self.nodes[node_id]
            }
            // Next is next sibling or parent
            else if let Some(parent) = node.parent_node() {
                let self_idx = parent
                    .children
                    .iter()
                    .position(|id| *id == node.id)
                    .unwrap();
                // Next is next sibling
                if let Some(sibling_id) = parent.children.get(self_idx + 1) {
                    look_in_children = true;
                    &self.nodes[*sibling_id]
                }
                // Next is parent
                else {
                    look_in_children = false;
                    node = parent;
                    continue;
                }
            }
            // Continue search from the root
            else {
                look_in_children = true;
                self.root_node()
            };

            if filter(next) {
                return Some(next.id);
            } else if next.id == start_id {
                return None;
            }

            node = next;
        }
    }

    pub fn focus_next_node(&mut self) -> Option<usize> {
        let focussed_node_id = self.get_focussed_node_id()?;
        let id = self.next_node(&self.nodes[focussed_node_id], |node| node.is_focussable())?;
        self.set_focus_to(id);
        Some(id)
    }

    pub fn set_focus_to(&mut self, focus_node_id: usize) -> bool {
        if Some(focus_node_id) == self.focus_node_id {
            return false;
        }

        println!("Focussed node {}", focus_node_id);

        // Remove focus from the old node
        if let Some(id) = self.focus_node_id {
            self.snapshot_node_and(id, |node| node.blur());
        }

        // Focus the new node
        self.snapshot_node_and(focus_node_id, |node| node.focus());

        self.focus_node_id = Some(focus_node_id);

        true
    }

    pub fn set_hover_to(&mut self, x: f32, y: f32) -> bool {
        let hit = self.hit(x, y);
        let hover_node_id = hit.map(|hit| hit.node_id);

        // Return early if the new node is the same as the already-hovered node
        if hover_node_id == self.hover_node_id {
            return false;
        }

        let mut maybe_id = self.hover_node_id;
        while let Some(id) = maybe_id {
            self.snapshot_node_and(id, |node| {
                node.unhover();
                maybe_id = node.layout_parent.get();
            });
        }

        let mut maybe_id = hover_node_id;
        while let Some(id) = maybe_id {
            self.snapshot_node_and(id, |node| {
                node.hover();
                maybe_id = node.layout_parent.get();
            });
        }

        self.hover_node_id = hover_node_id;

        true
    }

    pub fn get_hover_node_id(&self) -> Option<usize> {
        self.hover_node_id
    }

    pub fn set_viewport(&mut self, viewport: Viewport) {
        self.viewport = viewport;
        self.set_stylist_device(self.viewport.make_device());
    }

    pub fn get_viewport(&self) -> Viewport {
        self.viewport.clone()
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

    /// Ensure that the layout_children field is populated for all nodes
    pub fn resolve_layout_children(&mut self) {
        let root_node_id = self.root_node().id;
        resolve_layout_children_recursive(self, root_node_id);

        pub fn resolve_layout_children_recursive(doc: &mut Document, node_id: usize) {
            doc.nodes[node_id].is_inline_root = false;
            if let Some(element_data) = doc.nodes[node_id].element_data_mut() {
                element_data.take_inline_layout();
            }

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

        let root_element_id = taffy::NodeId::from(self.root_element().id);

        // println!("\n\nRESOLVE LAYOUT\n===========\n");

        taffy::compute_root_layout(self, root_element_id, available_space);
        taffy::round_layout(self, root_element_id);

        // println!("\n\n");
        // taffy::print_tree(self, root_node_id)
    }

    pub fn set_document(&mut self, _content: String) {}

    pub fn add_element(&mut self) {}

    pub fn get_cursor(&self) -> Option<CursorKind> {
        // todo: cache this on the node itself
        let node = &self.nodes[self.get_hover_node_id()?];

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

    /// Scroll a node by given x and y
    /// Will bubble scrolling up to parent node once it can no longer scroll further
    /// If we're already at the root node, bubbles scrolling up to the viewport
    pub fn scroll_node_by(&mut self, node_id: usize, x: f64, y: f64) {
        let Some(node) = self.nodes.get_mut(node_id) else {
            return;
        };

        let (can_x_scroll, can_y_scroll) = node
            .primary_styles()
            .map(|styles| {
                (
                    matches!(styles.clone_overflow_x(), Overflow::Scroll | Overflow::Auto),
                    matches!(styles.clone_overflow_y(), Overflow::Scroll | Overflow::Auto),
                )
            })
            .unwrap_or((false, false));

        let new_x = node.scroll_offset.x - x;
        let new_y = node.scroll_offset.y - y;

        let mut bubble_x = 0.0;
        let mut bubble_y = 0.0;

        let scroll_width = node.final_layout.scroll_width() as f64;
        let scroll_height = node.final_layout.scroll_height() as f64;

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

        if bubble_x != 0.0 || bubble_y != 0.0 {
            if let Some(parent) = node.parent {
                self.scroll_node_by(parent, bubble_x, bubble_y);
            } else {
                self.scroll_viewport_by(bubble_x, bubble_y);
            }
        }
    }

    /// Scroll the viewport by the given values
    pub fn scroll_viewport_by(&mut self, x: f64, y: f64) {
        let content_size = self.root_element().final_layout.size;
        let new_scroll = (self.viewport_scroll.x - x, self.viewport_scroll.y - y);
        let window_width = self.viewport.window_size.0 as f64 / self.viewport.scale() as f64;
        let window_height = self.viewport.window_size.1 as f64 / self.viewport.scale() as f64;
        self.viewport_scroll.x = f64::max(
            0.0,
            f64::min(new_scroll.0, content_size.width as f64 - window_width),
        );
        self.viewport_scroll.y = f64::max(
            0.0,
            f64::min(new_scroll.1, content_size.height as f64 - window_height),
        )
    }

    pub fn viewport_scroll(&self) -> kurbo::Point {
        self.viewport_scroll
    }

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
