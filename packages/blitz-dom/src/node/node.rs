use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};
use bitflags::bitflags;
use blitz_traits::events::{BlitzMouseButtonEvent, DomEventData, HitResult};
use keyboard_types::Modifiers;
use markup5ever::{LocalName, local_name};
use parley::Cluster;
use peniko::kurbo;
use selectors::matching::ElementSelectorFlags;
use slab::Slab;
use std::cell::{Cell, RefCell};
use std::fmt::Write;
use std::sync::atomic::AtomicBool;
use style::Atom;
use style::invalidation::element::restyle_hints::RestyleHint;
use style::properties::ComputedValues;
use style::properties::generated::longhands::position::computed_value::T as Position;
use style::selector_parser::{PseudoElement, RestyleDamage};
use style::stylesheets::UrlExtraData;
use style::values::computed::Display as StyloDisplay;
use style::values::specified::box_::{DisplayInside, DisplayOutside};
use style::{data::ElementData as StyloElementData, shared_lock::SharedRwLock};
use style_dom::ElementState;
use style_traits::values::ToCss;
use taffy::{
    Cache,
    prelude::{Layout, Style},
};

use super::{Attribute, ElementData};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayOuter {
    Block,
    Inline,
    None,
}

bitflags! {
    #[derive(Clone, Copy, PartialEq)]
    pub struct NodeFlags: u32 {
        /// Whether the node is the root node of an Inline Formatting Context
        const IS_INLINE_ROOT = 0b00000001;
        /// Whether the node is the root node of an Table formatting context
        const IS_TABLE_ROOT = 0b00000010;
        /// Whether the node is "in the document" (~= has a parent and isn't a template node)
        const IS_IN_DOCUMENT = 0b00000100;
    }
}

impl NodeFlags {
    #[inline(always)]
    pub fn is_inline_root(&self) -> bool {
        self.contains(Self::IS_INLINE_ROOT)
    }

    #[inline(always)]
    pub fn is_table_root(&self) -> bool {
        self.contains(Self::IS_TABLE_ROOT)
    }

    #[inline(always)]
    pub fn is_in_document(&self) -> bool {
        self.contains(Self::IS_IN_DOCUMENT)
    }

    #[inline(always)]
    pub fn reset_construction_flags(&mut self) {
        self.remove(Self::IS_INLINE_ROOT);
        self.remove(Self::IS_TABLE_ROOT);
    }
}

pub struct Node {
    // The actual tree we belong to. This is unsafe!!
    tree: *mut Slab<Node>,

    /// Our Id
    pub id: usize,
    /// Our parent's ID
    pub parent: Option<usize>,
    // What are our children?
    pub children: Vec<usize>,
    /// Our parent in the layout hierachy: a separate list that includes anonymous collections of inline elements
    pub layout_parent: Cell<Option<usize>>,
    /// A separate child list that includes anonymous collections of inline elements
    pub layout_children: RefCell<Option<Vec<usize>>>,
    /// The same as layout_children, but sorted by z-index
    pub paint_children: RefCell<Option<Vec<usize>>>,

    // Flags
    pub flags: NodeFlags,

    /// Node type (Element, TextNode, etc) specific data
    pub data: NodeData,

    // This little bundle of joy is our style data from stylo and a lock guard that allows access to it
    // TODO: See if guard can be hoisted to a higher level
    pub stylo_element_data: AtomicRefCell<Option<StyloElementData>>,
    pub selector_flags: AtomicRefCell<ElementSelectorFlags>,
    pub guard: SharedRwLock,
    pub element_state: ElementState,

    // Pseudo element nodes
    pub before: Option<usize>,
    pub after: Option<usize>,

    // Taffy layout data:
    pub style: Style<Atom>,
    pub has_snapshot: bool,
    pub snapshot_handled: AtomicBool,
    pub display_constructed_as: StyloDisplay,
    pub cache: Cache,
    pub unrounded_layout: Layout,
    pub final_layout: Layout,
    pub scroll_offset: kurbo::Point,
}

unsafe impl Send for Node {}
unsafe impl Sync for Node {}

impl Node {
    pub(crate) fn new(
        tree: *mut Slab<Node>,
        id: usize,
        guard: SharedRwLock,
        data: NodeData,
    ) -> Self {
        Self {
            tree,

            id,
            parent: None,
            children: vec![],
            layout_parent: Cell::new(None),
            layout_children: RefCell::new(None),
            paint_children: RefCell::new(None),

            flags: NodeFlags::empty(),
            data,

            stylo_element_data: Default::default(),
            selector_flags: AtomicRefCell::new(ElementSelectorFlags::empty()),
            guard,
            element_state: ElementState::empty(),

            before: None,
            after: None,

            style: Default::default(),
            has_snapshot: false,
            snapshot_handled: AtomicBool::new(false),
            display_constructed_as: StyloDisplay::Block,
            cache: Cache::new(),
            unrounded_layout: Layout::new(),
            final_layout: Layout::new(),
            scroll_offset: kurbo::Point::ZERO,
        }
    }

    pub fn pe_by_index(&self, index: usize) -> Option<usize> {
        match index {
            0 => self.after,
            1 => self.before,
            _ => panic!("Invalid pseudo element index"),
        }
    }

    pub fn set_pe_by_index(&mut self, index: usize, value: Option<usize>) {
        match index {
            0 => self.after = value,
            1 => self.before = value,
            _ => panic!("Invalid pseudo element index"),
        }
    }

    pub(crate) fn display_style(&self) -> Option<StyloDisplay> {
        Some(self.primary_styles().as_ref()?.clone_display())
    }

    pub fn is_or_contains_block(&self) -> bool {
        let style = self.primary_styles();
        let style = style.as_ref();

        // Ignore out-of-flow items
        let position = style
            .map(|s| s.clone_position())
            .unwrap_or(Position::Relative);
        let is_in_flow = matches!(
            position,
            Position::Static | Position::Relative | Position::Sticky
        );
        if !is_in_flow {
            return false;
        }
        let display = style
            .map(|s| s.clone_display())
            .unwrap_or(StyloDisplay::inline());
        match display.outside() {
            DisplayOutside::None => false,
            DisplayOutside::Block => true,
            _ => {
                if display.inside() == DisplayInside::Flow {
                    self.children
                        .iter()
                        .copied()
                        .any(|child_id| self.tree()[child_id].is_or_contains_block())
                } else {
                    false
                }
            }
        }
    }

    pub fn is_focussable(&self) -> bool {
        self.data
            .downcast_element()
            .map(|el| el.is_focussable)
            .unwrap_or(false)
    }

    pub fn set_restyle_hint(&self, hint: RestyleHint) {
        if let Some(element_data) = self.stylo_element_data.borrow_mut().as_mut() {
            element_data.hint.insert(hint);
        }
    }

    pub fn damage_mut(&self) -> Option<AtomicRefMut<'_, RestyleDamage>> {
        let element_data = self.stylo_element_data.borrow_mut();
        #[allow(clippy::manual_map, reason = "false positive")]
        match *element_data {
            Some(_) => Some(AtomicRefMut::map(
                element_data,
                |data: &mut Option<StyloElementData>| &mut data.as_mut().unwrap().damage,
            )),
            None => None,
        }
    }

    pub fn damage(&mut self) -> Option<RestyleDamage> {
        self.stylo_element_data
            .get_mut()
            .as_ref()
            .map(|data| data.damage)
    }

    pub fn set_damage(&self, damage: RestyleDamage) {
        if let Some(data) = self.stylo_element_data.borrow_mut().as_mut() {
            data.damage = damage;
        }
    }

    pub fn insert_damage(&mut self, damage: RestyleDamage) {
        if let Some(data) = self.stylo_element_data.get_mut().as_mut() {
            data.damage |= damage;
        }
    }

    pub fn remove_damage(&self, damage: RestyleDamage) {
        if let Some(data) = self.stylo_element_data.borrow_mut().as_mut() {
            data.damage.remove(damage);
        }
    }

    pub fn clear_damage_mut(&mut self) {
        if let Some(data) = self.stylo_element_data.get_mut() {
            data.damage = RestyleDamage::empty();
        }
    }

    pub fn hover(&mut self) {
        self.element_state.insert(ElementState::HOVER);
        self.set_restyle_hint(RestyleHint::restyle_subtree());
    }

    pub fn unhover(&mut self) {
        self.element_state.remove(ElementState::HOVER);
        self.set_restyle_hint(RestyleHint::restyle_subtree());
    }

    pub fn is_hovered(&self) -> bool {
        self.element_state.contains(ElementState::HOVER)
    }

    pub fn focus(&mut self) {
        self.element_state
            .insert(ElementState::FOCUS | ElementState::FOCUSRING);
        self.set_restyle_hint(RestyleHint::restyle_subtree());
    }

    pub fn blur(&mut self) {
        self.element_state
            .remove(ElementState::FOCUS | ElementState::FOCUSRING);
        self.set_restyle_hint(RestyleHint::restyle_subtree());
    }

    pub fn is_focussed(&self) -> bool {
        self.element_state.contains(ElementState::FOCUS)
    }

    pub fn active(&mut self) {
        self.element_state.insert(ElementState::ACTIVE);
        self.set_restyle_hint(RestyleHint::restyle_subtree());
    }

    pub fn unactive(&mut self) {
        self.element_state.remove(ElementState::ACTIVE);
        self.set_restyle_hint(RestyleHint::restyle_subtree());
    }

    pub fn is_active(&self) -> bool {
        self.element_state.contains(ElementState::ACTIVE)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NodeKind {
    Document,
    Element,
    AnonymousBlock,
    Text,
    Comment,
}

/// The different kinds of nodes in the DOM.
#[derive(Debug, Clone)]
pub enum NodeData {
    /// The `Document` itself - the root node of a HTML document.
    Document,

    /// An element with attributes.
    Element(ElementData),

    /// An anonymous block box
    AnonymousBlock(ElementData),

    /// A text node.
    Text(TextNodeData),

    /// A comment.
    Comment,
    // Comment { contents: String },

    // /// A `DOCTYPE` with name, public id, and system id. See
    // /// [document type declaration on wikipedia][https://en.wikipedia.org/wiki/Document_type_declaration]
    // Doctype { name: String, public_id: String, system_id: String },

    // /// A Processing instruction.
    // ProcessingInstruction { target: String, contents: String },
}

impl NodeData {
    pub fn downcast_element(&self) -> Option<&ElementData> {
        match self {
            Self::Element(data) => Some(data),
            Self::AnonymousBlock(data) => Some(data),
            _ => None,
        }
    }

    pub fn downcast_element_mut(&mut self) -> Option<&mut ElementData> {
        match self {
            Self::Element(data) => Some(data),
            Self::AnonymousBlock(data) => Some(data),
            _ => None,
        }
    }

    pub fn is_element_with_tag_name(&self, name: &impl PartialEq<LocalName>) -> bool {
        let Some(elem) = self.downcast_element() else {
            return false;
        };
        *name == elem.name.local
    }

    pub fn attrs(&self) -> Option<&[Attribute]> {
        Some(&self.downcast_element()?.attrs)
    }

    pub fn attr(&self, name: impl PartialEq<LocalName>) -> Option<&str> {
        self.downcast_element()?.attr(name)
    }

    pub fn has_attr(&self, name: impl PartialEq<LocalName>) -> bool {
        self.downcast_element()
            .is_some_and(|elem| elem.has_attr(name))
    }

    pub fn kind(&self) -> NodeKind {
        match self {
            NodeData::Document => NodeKind::Document,
            NodeData::Element(_) => NodeKind::Element,
            NodeData::AnonymousBlock(_) => NodeKind::AnonymousBlock,
            NodeData::Text(_) => NodeKind::Text,
            NodeData::Comment => NodeKind::Comment,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TextNodeData {
    /// The textual content of the text node
    pub content: String,
}

impl TextNodeData {
    pub fn new(content: String) -> Self {
        Self { content }
    }
}

/*
-> Computed styles
-> Layout
-----> Needs to happen only when styles are computed
*/

// type DomRefCell<T> = RefCell<T>;

// pub struct DomData {
//     // ... we can probs just get away with using the html5ever types directly. basically just using the servo dom, but without the bindings
//     local_name: html5ever::LocalName,
//     tag_name: html5ever::QualName,
//     namespace: html5ever::Namespace,
//     prefix: DomRefCell<Option<html5ever::Prefix>>,
//     attrs: DomRefCell<Vec<Attr>>,
//     // attrs: DomRefCell<Vec<Dom<Attr>>>,
//     id_attribute: DomRefCell<Option<Atom>>,
//     is: DomRefCell<Option<LocalName>>,
//     // style_attribute: DomRefCell<Option<Arc<Locked<PropertyDeclarationBlock>>>>,
//     // attr_list: MutNullableDom<NamedNodeMap>,
//     // class_list: MutNullableDom<DOMTokenList>,
//     state: Cell<ElementState>,
// }

impl Node {
    pub fn tree(&self) -> &Slab<Node> {
        unsafe { &*self.tree }
    }

    #[track_caller]
    pub fn with(&self, id: usize) -> &Node {
        self.tree().get(id).unwrap()
    }

    pub fn print_tree(&self, level: usize) {
        println!(
            "{} {} {:?} {} {:?}",
            "  ".repeat(level),
            self.id,
            self.parent,
            self.node_debug_str().replace('\n', ""),
            self.children
        );
        // println!("{} {:?}", "  ".repeat(level), self.children);
        for child_id in self.children.iter() {
            let child = self.with(*child_id);
            child.print_tree(level + 1)
        }
    }

    // Get the index of the current node in the parents child list
    pub fn index_of_child(&self, child_id: usize) -> Option<usize> {
        self.children.iter().position(|id| *id == child_id)
    }

    // Get the index of the current node in the parents child list
    pub fn child_index(&self) -> Option<usize> {
        self.tree()[self.parent?]
            .children
            .iter()
            .position(|id| *id == self.id)
    }

    // Get the nth node in the parents child list
    pub fn forward(&self, n: usize) -> Option<&Node> {
        let child_idx = self.child_index().unwrap_or(0);
        self.tree()[self.parent?]
            .children
            .get(child_idx + n)
            .map(|id| self.with(*id))
    }

    pub fn backward(&self, n: usize) -> Option<&Node> {
        let child_idx = self.child_index().unwrap_or(0);
        if child_idx < n {
            return None;
        }

        self.tree()[self.parent?]
            .children
            .get(child_idx - n)
            .map(|id| self.with(*id))
    }

    pub fn is_element(&self) -> bool {
        matches!(self.data, NodeData::Element { .. })
    }

    pub fn is_anonymous(&self) -> bool {
        matches!(self.data, NodeData::AnonymousBlock { .. })
    }

    pub fn is_text_node(&self) -> bool {
        matches!(self.data, NodeData::Text { .. })
    }

    pub fn element_data(&self) -> Option<&ElementData> {
        match self.data {
            NodeData::Element(ref data) => Some(data),
            NodeData::AnonymousBlock(ref data) => Some(data),
            _ => None,
        }
    }

    pub fn element_data_mut(&mut self) -> Option<&mut ElementData> {
        match self.data {
            NodeData::Element(ref mut data) => Some(data),
            NodeData::AnonymousBlock(ref mut data) => Some(data),
            _ => None,
        }
    }

    pub fn text_data(&self) -> Option<&TextNodeData> {
        match self.data {
            NodeData::Text(ref data) => Some(data),
            _ => None,
        }
    }

    pub fn text_data_mut(&mut self) -> Option<&mut TextNodeData> {
        match self.data {
            NodeData::Text(ref mut data) => Some(data),
            _ => None,
        }
    }

    pub fn node_debug_str(&self) -> String {
        let mut s = String::new();

        match &self.data {
            NodeData::Document => write!(s, "DOCUMENT"),
            // NodeData::Doctype { name, .. } => write!(s, "DOCTYPE {name}"),
            NodeData::Text(data) => {
                let bytes = data.content.as_bytes();
                write!(
                    s,
                    "TEXT {}",
                    &std::str::from_utf8(bytes.split_at(10.min(bytes.len())).0)
                        .unwrap_or("INVALID UTF8")
                )
            }
            NodeData::Comment => write!(
                s,
                "COMMENT",
                // &std::str::from_utf8(data.contents.as_bytes().split_at(10).0).unwrap_or("INVALID UTF8")
            ),
            NodeData::AnonymousBlock(_) => write!(s, "AnonymousBlock"),
            NodeData::Element(data) => {
                let name = &data.name;
                let class = self.attr(local_name!("class")).unwrap_or("");
                let display = self.display_constructed_as.to_css_string();
                if !class.is_empty() {
                    write!(s, "<{} class=\"{}\"> ({})", name.local, class, display)
                } else {
                    write!(s, "<{}> ({})", name.local, display)
                }
            } // NodeData::ProcessingInstruction { .. } => write!(s, "ProcessingInstruction"),
        }
        .unwrap();
        s
    }

    pub fn outer_html(&self) -> String {
        let mut output = String::new();
        self.write_outer_html(&mut output);
        output
    }

    pub fn write_outer_html(&self, writer: &mut String) {
        let has_children = !self.children.is_empty();
        let current_color = self
            .primary_styles()
            .map(|style| style.clone_color())
            .map(|color| color.to_css_string());

        match &self.data {
            NodeData::Document => {}
            NodeData::Comment => {}
            NodeData::AnonymousBlock(_) => {}
            // NodeData::Doctype { name, .. } => write!(s, "DOCTYPE {name}"),
            NodeData::Text(data) => {
                writer.push_str(data.content.as_str());
            }
            NodeData::Element(data) => {
                writer.push('<');
                writer.push_str(&data.name.local);

                for attr in data.attrs() {
                    writer.push(' ');
                    writer.push_str(&attr.name.local);
                    writer.push_str("=\"");
                    #[allow(clippy::unnecessary_unwrap)] // Convert to if-let chain once stabilised
                    if current_color.is_some() && attr.value.contains("currentColor") {
                        writer.push_str(
                            &attr
                                .value
                                .replace("currentColor", current_color.as_ref().unwrap()),
                        );
                    } else {
                        writer.push_str(&attr.value);
                    }
                    writer.push('"');
                }
                if !has_children {
                    writer.push_str(" /");
                }
                writer.push('>');

                if has_children {
                    for &child_id in &self.children {
                        self.tree()[child_id].write_outer_html(writer);
                    }

                    writer.push_str("</");
                    writer.push_str(&data.name.local);
                    writer.push('>');
                }
            }
        }
    }

    pub fn attrs(&self) -> Option<&[Attribute]> {
        Some(&self.element_data()?.attrs)
    }

    pub fn attr(&self, name: LocalName) -> Option<&str> {
        let attr = self.attrs()?.iter().find(|id| id.name.local == name)?;
        Some(&attr.value)
    }

    pub fn primary_styles(&self) -> Option<AtomicRef<'_, ComputedValues>> {
        let stylo_element_data = self.stylo_element_data.borrow();
        if stylo_element_data
            .as_ref()
            .and_then(|d| d.styles.get_primary())
            .is_some()
        {
            Some(AtomicRef::map(
                stylo_element_data,
                |data: &Option<StyloElementData>| -> &ComputedValues {
                    data.as_ref().unwrap().styles.get_primary().unwrap()
                },
            ))
        } else {
            None
        }
    }

    pub fn text_content(&self) -> String {
        let mut out = String::new();
        self.write_text_content(&mut out);
        out
    }

    fn write_text_content(&self, out: &mut String) {
        match &self.data {
            NodeData::Text(data) => {
                out.push_str(&data.content);
            }
            NodeData::Element(..) | NodeData::AnonymousBlock(..) => {
                for child_id in self.children.iter() {
                    self.with(*child_id).write_text_content(out);
                }
            }
            _ => {}
        }
    }

    pub fn flush_style_attribute(&mut self, url_extra_data: &UrlExtraData) {
        if let NodeData::Element(ref mut elem_data) = self.data {
            elem_data.flush_style_attribute(&self.guard, url_extra_data);
        }
    }

    pub fn order(&self) -> i32 {
        self.primary_styles()
            .map(|s| match s.pseudo() {
                Some(PseudoElement::Before) => i32::MIN,
                Some(PseudoElement::After) => i32::MAX,
                _ => s.clone_order(),
            })
            .unwrap_or(0)
    }

    pub fn z_index(&self) -> i32 {
        self.primary_styles()
            .map(|s| s.clone_z_index().integer_or(0))
            .unwrap_or(0)
    }

    /// Takes an (x, y) position (relative to the *parent's* top-left corner) and returns:
    ///    - None if the position is outside of this node's bounds
    ///    - Some(HitResult) if the position is within the node but doesn't match any children
    ///    - The result of recursively calling child.hit() on the the child element that is
    ///      positioned at that position if there is one.
    ///
    /// TODO: z-index
    /// (If multiple children are positioned at the position then a random one will be recursed into)
    pub fn hit(&self, x: f32, y: f32) -> Option<HitResult> {
        let mut x = x - self.final_layout.location.x + self.scroll_offset.x as f32;
        let mut y = y - self.final_layout.location.y + self.scroll_offset.y as f32;

        let size = self.final_layout.size;
        let matches_self = !(x < 0.0
            || x > size.width + self.scroll_offset.x as f32
            || y < 0.0
            || y > size.height + self.scroll_offset.y as f32);

        let content_size = self.final_layout.content_size;
        let matches_content = !(x < 0.0
            || x > content_size.width + self.scroll_offset.x as f32
            || y < 0.0
            || y > content_size.height + self.scroll_offset.y as f32);

        if !matches_self && !matches_content {
            return None;
        }

        if self.flags.is_inline_root() {
            let content_box_offset = taffy::Point {
                x: self.final_layout.padding.left + self.final_layout.border.left,
                y: self.final_layout.padding.top + self.final_layout.border.top,
            };
            x -= content_box_offset.x;
            y -= content_box_offset.y;
        }

        // Call `.hit()` on each child in turn. If any return `Some` then return that value. Else return `Some(self.id).
        self.paint_children
            .borrow()
            .iter()
            .flatten()
            .rev()
            .find_map(|&i| self.with(i).hit(x, y))
            .or_else(|| {
                if self.flags.is_inline_root() {
                    let element_data = &self.element_data().unwrap();
                    let layout = &element_data.inline_layout_data.as_ref().unwrap().layout;
                    let scale = layout.scale();

                    Cluster::from_point(layout, x * scale, y * scale).and_then(|(cluster, _)| {
                        let style_index = cluster.glyphs().next()?.style_index();
                        let node_id = layout.styles()[style_index].brush.id;
                        Some(HitResult { node_id, x, y })
                    })
                } else {
                    None
                }
            })
            .or(Some(HitResult {
                node_id: self.id,
                x,
                y,
            })
            .filter(|_| matches_self))
    }

    /// Computes the Document-relative coordinates of the Node
    pub fn absolute_position(&self, x: f32, y: f32) -> taffy::Point<f32> {
        let x = x + self.final_layout.location.x - self.scroll_offset.x as f32;
        let y = y + self.final_layout.location.y - self.scroll_offset.y as f32;

        // Recurse up the layout hierarchy
        self.layout_parent
            .get()
            .map(|i| self.with(i).absolute_position(x, y))
            .unwrap_or(taffy::Point { x, y })
    }

    /// Creates a synthetic click event
    pub fn synthetic_click_event(&self, mods: Modifiers) -> DomEventData {
        DomEventData::Click(self.synthetic_click_event_data(mods))
    }

    pub fn synthetic_click_event_data(&self, mods: Modifiers) -> BlitzMouseButtonEvent {
        let absolute_position = self.absolute_position(0.0, 0.0);
        let x = absolute_position.x + (self.final_layout.size.width / 2.0);
        let y = absolute_position.y + (self.final_layout.size.height / 2.0);

        BlitzMouseButtonEvent {
            x,
            y,
            mods,
            button: Default::default(),
            buttons: Default::default(),
        }
    }
}

/// It might be wrong to expose this since what does *equality* mean outside the dom?
impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Node {}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // FIXME: update to reflect changes to fields
        f.debug_struct("NodeData")
            .field("parent", &self.parent)
            .field("id", &self.id)
            .field("is_inline_root", &self.flags.is_inline_root())
            .field("children", &self.children)
            .field("layout_children", &self.layout_children.borrow())
            // .field("style", &self.style)
            .field("node", &self.data)
            .field("stylo_element_data", &self.stylo_element_data)
            // .field("unrounded_layout", &self.unrounded_layout)
            // .field("final_layout", &self.final_layout)
            .finish()
    }
}
