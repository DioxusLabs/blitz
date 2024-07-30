use atomic_refcell::{AtomicRef, AtomicRefCell};
use html5ever::{local_name, LocalName, QualName};
use image::DynamicImage;
use selectors::matching::QuirksMode;
use slab::Slab;
use std::cell::RefCell;
use std::fmt::Write;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use style::invalidation::element::restyle_hints::RestyleHint;
use style::values::computed::Display;
use style::values::specified::box_::{DisplayInside, DisplayOutside};
use style_dom::ElementState;
// use string_cache::Atom;
use parley;
use style::properties::ComputedValues;
use style::stylesheets::UrlExtraData;
use style::Atom;
use style::{
    data::ElementData,
    properties::{parse_style_attribute, PropertyDeclarationBlock},
    servo_arc::Arc as ServoArc,
    shared_lock::{Locked, SharedRwLock},
    stylesheets::CssRuleType,
};
use taffy::{
    prelude::{Layout, Style},
    Cache,
};
use url::Url;

use crate::events::{EventListener, HitResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayOuter {
    Block,
    Inline,
    None,
}

// todo: might be faster to migrate this to ecs and split apart at a different boundary
pub struct Node {
    // The actual tree we belong to. This is unsafe!!
    pub tree: *mut Slab<Node>,

    /// Our parent's ID
    pub parent: Option<usize>,
    /// Our Id
    pub id: usize,
    // Which child are we in our parent?
    pub child_idx: usize,
    // What are our children?
    pub children: Vec<usize>,
    /// A separate child list that includes anonymous collections of inline elements
    pub layout_children: RefCell<Option<Vec<usize>>>,

    /// Node type (Element, TextNode, etc) specific data
    pub raw_dom_data: NodeData,

    // This little bundle of joy is our style data from stylo and a lock guard that allows access to it
    // TODO: See if guard can be hoisted to a higher level
    pub stylo_element_data: AtomicRefCell<Option<ElementData>>,
    pub guard: SharedRwLock,
    pub element_state: ElementState,

    // Taffy layout data:
    pub style: Style,
    pub hidden: bool,
    pub is_hovered: bool,
    pub has_snapshot: bool,
    pub snapshot_handled: AtomicBool,
    pub display_outer: DisplayOuter,
    pub cache: Cache,
    pub unrounded_layout: Layout,
    pub final_layout: Layout,
    pub listeners: Vec<EventListener>,

    // Inline layout data
    pub is_inline_root: bool,
}

impl Node {
    pub fn new(tree: *mut Slab<Node>, id: usize, guard: SharedRwLock, data: NodeData) -> Self {
        Self {
            tree,

            id,
            parent: None,
            children: vec![],
            layout_children: RefCell::new(None),
            child_idx: 0,

            raw_dom_data: data,
            stylo_element_data: Default::default(),
            guard,
            element_state: ElementState::empty(),

            style: Default::default(),
            hidden: false,
            is_hovered: false,
            has_snapshot: false,
            snapshot_handled: AtomicBool::new(false),
            display_outer: DisplayOuter::Block,
            cache: Cache::new(),
            unrounded_layout: Layout::new(),
            final_layout: Layout::new(),
            listeners: Default::default(),
            is_inline_root: false,
        }
    }

    pub(crate) fn display_style(&self) -> Option<Display> {
        // if self.is_text_node() {
        //     return Some(Display::inline())
        // }

        Some(
            self.stylo_element_data
                .borrow()
                .as_ref()?
                .styles
                .primary
                .as_ref()?
                .get_box()
                .display,
        )
    }

    pub fn is_or_contains_block(&self) -> bool {
        let display = self.display_style().unwrap_or(Display::inline());

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
        self.raw_dom_data
            .downcast_element()
            .map(|el| el.is_focussable)
            .unwrap_or(false)
    }

    pub fn set_restyle_hint(&mut self, hint: RestyleHint) {
        if let Some(element_data) = self.stylo_element_data.borrow_mut().as_mut() {
            element_data.hint.insert(hint);
        }
    }

    pub fn hover(&mut self) {
        self.is_hovered = true;
        self.element_state.insert(ElementState::HOVER);
        self.set_restyle_hint(RestyleHint::RESTYLE_SELF);
    }

    pub fn unhover(&mut self) {
        self.is_hovered = false;
        self.element_state.remove(ElementState::HOVER);
        self.set_restyle_hint(RestyleHint::RESTYLE_SELF);
    }

    pub fn focus(&mut self) {
        self.element_state
            .insert(ElementState::FOCUS | ElementState::FOCUSRING);
        self.set_restyle_hint(RestyleHint::RESTYLE_SELF);
    }

    pub fn blur(&mut self) {
        self.element_state
            .remove(ElementState::FOCUS | ElementState::FOCUSRING);
        self.set_restyle_hint(RestyleHint::RESTYLE_SELF);
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
    Element(ElementNodeData),

    /// An anonymous block box
    AnonymousBlock(ElementNodeData),

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
    pub fn downcast_element(&self) -> Option<&ElementNodeData> {
        match self {
            Self::Element(data) => Some(data),
            Self::AnonymousBlock(data) => Some(data),
            _ => None,
        }
    }

    pub fn downcast_element_mut(&mut self) -> Option<&mut ElementNodeData> {
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

/// A tag attribute, e.g. `class="test"` in `<div class="test" ...>`.
///
/// The namespace on the attribute name is almost always ns!("").
/// The tokenizer creates all attributes this way, but the tree
/// builder will adjust certain attribute names inside foreign
/// content (MathML, SVG).
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug)]
pub struct Attribute {
    /// The name of the attribute (e.g. the `class` in `<div class="test">`)
    pub name: QualName,
    /// The value of the attribute (e.g. the `"test"` in `<div class="test">`)
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct ElementNodeData {
    /// The elements tag name, namespace and prefix
    pub name: QualName,

    /// The elements id attribute parsed as an atom (if it has one)
    pub id: Option<Atom>,

    /// The element's attributes
    pub attrs: Vec<Attribute>,

    /// Whether the element is focussable
    pub is_focussable: bool,

    /// The element's parsed style attribute (used by stylo)
    pub style_attribute: Option<ServoArc<Locked<PropertyDeclarationBlock>>>,

    /// Heterogeneous data that depends on the element's type.
    /// For example:
    ///   - The image data for \<img\> elements.
    ///   - The parley Layout for inline roots.
    ///   - The text editor for input/textarea elements
    pub node_specific_data: NodeSpecificData,

    /// The element's template contents (\<template\> elements only)
    pub template_contents: Option<usize>,
    // /// Whether the node is a [HTML integration point] (https://html.spec.whatwg.org/multipage/#html-integration-point)
    // pub mathml_annotation_xml_integration_point: bool,
}

impl ElementNodeData {
    pub fn new(name: QualName, attrs: Vec<Attribute>) -> Self {
        let id_attr_atom = attrs
            .iter()
            .find(|attr| &attr.name.local == "id")
            .map(|attr| attr.value.as_ref())
            .map(|value: &str| Atom::from(value));

        let mut data = ElementNodeData {
            name,
            id: id_attr_atom,
            attrs,
            is_focussable: false,
            style_attribute: Default::default(),
            node_specific_data: NodeSpecificData::None,
            template_contents: None,
            // listeners: FxHashSet::default(),
        };
        data.flush_is_focussable();
        data
    }

    pub fn attrs(&self) -> &[Attribute] {
        &self.attrs
    }

    pub fn attr(&self, name: impl PartialEq<LocalName>) -> Option<&str> {
        let attr = self.attrs.iter().find(|attr| name == attr.name.local)?;
        Some(&attr.value)
    }

    pub fn attr_parsed<T: FromStr>(&self, name: impl PartialEq<LocalName>) -> Option<T> {
        let attr = self.attrs.iter().find(|attr| name == attr.name.local)?;
        attr.value.parse::<T>().ok()
    }

    pub fn image_data(&self) -> Option<&ImageData> {
        match self.node_specific_data {
            NodeSpecificData::Image(ref data) => Some(data),
            _ => None,
        }
    }

    pub fn image_data_mut(&mut self) -> Option<&mut ImageData> {
        match self.node_specific_data {
            NodeSpecificData::Image(ref mut data) => Some(data),
            _ => None,
        }
    }

    pub fn svg_data(&self) -> Option<&usvg::Tree> {
        match self.node_specific_data {
            NodeSpecificData::Svg(ref data) => Some(data),
            _ => None,
        }
    }

    pub fn svg_data_mut(&mut self) -> Option<&mut usvg::Tree> {
        match self.node_specific_data {
            NodeSpecificData::Svg(ref mut data) => Some(data),
            _ => None,
        }
    }

    pub fn text_input_data(&self) -> Option<&TextInputData> {
        match self.node_specific_data {
            NodeSpecificData::TextInput(ref data) => Some(data),
            _ => None,
        }
    }

    pub fn text_input_data_mut(&mut self) -> Option<&mut TextInputData> {
        match self.node_specific_data {
            NodeSpecificData::TextInput(ref mut data) => Some(data),
            _ => None,
        }
    }

    pub fn inline_layout_data(&self) -> Option<&TextLayout> {
        match self.node_specific_data {
            NodeSpecificData::InlineRoot(ref data) => Some(data),
            _ => None,
        }
    }

    pub fn inline_layout_data_mut(&mut self) -> Option<&mut TextLayout> {
        match self.node_specific_data {
            NodeSpecificData::InlineRoot(ref mut data) => Some(data),
            _ => None,
        }
    }

    pub fn flush_is_focussable(&mut self) {
        let disabled: bool = self.attr_parsed(local_name!("disabled")).unwrap_or(false);
        let tabindex: Option<i32> = self.attr_parsed(local_name!("tabindex"));

        self.is_focussable = !disabled
            && match tabindex {
                Some(index) => index >= 0,
                None => {
                    // Some focusable HTML elements have a default tabindex value of 0 set under the hood by the user agent.
                    // These elements are:
                    //   - <a> or <area> with href attribute
                    //   - <button>, <frame>, <iframe>, <input>, <object>, <select>, <textarea>, and SVG <a> element
                    //   - <summary> element that provides summary for a <details> element.

                    if [local_name!("a"), local_name!("area")].contains(&self.name.local) {
                        self.attr(local_name!("href")).is_some()
                    } else {
                        const DEFAULT_FOCUSSABLE_ELEMENTS: [LocalName; 6] = [
                            local_name!("button"),
                            local_name!("input"),
                            local_name!("select"),
                            local_name!("textarea"),
                            local_name!("frame"),
                            local_name!("iframe"),
                        ];
                        DEFAULT_FOCUSSABLE_ELEMENTS.contains(&self.name.local)
                    }
                }
            }
    }

    pub fn flush_style_attribute(&mut self, guard: &SharedRwLock) {
        self.style_attribute = self.attr(local_name!("style")).map(|style_str| {
            let url = UrlExtraData::from(
                "data:text/css;charset=utf-8;base64,"
                    .parse::<Url>()
                    .unwrap(),
            );

            ServoArc::new(guard.wrap(parse_style_attribute(
                style_str,
                &url,
                None,
                QuirksMode::NoQuirks,
                CssRuleType::Style,
            )))
        });
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ImageData {
    /// The raw image data
    pub image: Arc<DynamicImage>,
    /// The resized image data (for the most recent size it's been displayed at)
    pub resized_image: RefCell<Option<Arc<peniko::Image>>>,
}

impl ImageData {
    pub fn new(image: Arc<DynamicImage>) -> Self {
        Self {
            image,
            resized_image: RefCell::new(None),
        }
    }
}

#[derive(Clone)]
pub struct TextInputData {
    /// A parley TextEditor instance
    pub editor: Box<parley::editor::TextEditor<String>>,
    /// Whether the input is a singleline or multiline input
    pub is_multiline: bool,
}

impl TextInputData {
    pub fn new(text: String, text_size: f32, is_multiline: bool) -> Self {
        let editor = Box::new(parley::editor::TextEditor::new(text, text_size));
        Self {
            editor,
            is_multiline,
        }
    }
}

/// Heterogeneous data that depends on the element's type.
#[derive(Clone)]
pub enum NodeSpecificData {
    /// The element's image content (\<img\> element's only)
    Image(ImageData),
    /// The element's image content (\<img\> element's only)
    Svg(usvg::Tree),
    /// Parley text layout (elements with inline inner display mode only)
    InlineRoot(Box<TextLayout>),
    /// Parley text editor (text inputs)
    TextInput(TextInputData),
    /// No data (for nodes that don't need any node-specific data)
    None,
}

impl std::fmt::Debug for NodeSpecificData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeSpecificData::Image(_) => f.write_str("NodeSpecificData::Image"),
            NodeSpecificData::Svg(_) => f.write_str("NodeSpecificData::Svg"),
            NodeSpecificData::InlineRoot(_) => f.write_str("NodeSpecificData::InlineRoot"),
            NodeSpecificData::TextInput(_) => f.write_str("NodeSpecificData::TextInput"),
            NodeSpecificData::None => f.write_str("NodeSpecificData::None"),
        }
    }
}

impl NodeSpecificData {
    pub fn take_inline_layout(&mut self) -> Option<Box<TextLayout>> {
        let taken = std::mem::take(self);
        match taken {
            Self::InlineRoot(data) => Some(data),
            _ => {
                *self = taken;
                None
            }
        }
    }
}

impl Default for NodeSpecificData {
    fn default() -> Self {
        Self::None
    }
}

pub type TextBrush = parley::editor::TextBrush;

#[derive(Clone)]
pub struct TextLayout {
    pub text: String,
    pub layout: parley::layout::Layout<TextBrush>,
}

impl std::fmt::Debug for TextLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TextLayout")
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

    pub fn with(&self, id: usize) -> &Node {
        self.tree().get(id).unwrap()
    }

    pub fn print_tree(&self, level: usize) {
        println!(
            "{} {} {:?} {} {} {:?}",
            "  ".repeat(level),
            self.id,
            self.parent,
            self.child_idx,
            self.node_debug_str().replace('\n', ""),
            self.children
        );
        // println!("{} {:?}", "  ".repeat(level), self.children);
        for child_id in self.children.iter() {
            let child = self.with(*child_id);
            child.print_tree(level + 1)
        }
    }

    // Get the nth node in the parents child list
    pub fn forward(&self, n: usize) -> Option<&Node> {
        self.tree()[self.parent?]
            .children
            .get(self.child_idx + n)
            .map(|id| self.with(*id))
    }

    pub fn backward(&self, n: usize) -> Option<&Node> {
        if self.child_idx < n {
            return None;
        }

        self.tree()[self.parent?]
            .children
            .get(self.child_idx - n)
            .map(|id| self.with(*id))
    }

    pub fn is_element(&self) -> bool {
        matches!(self.raw_dom_data, NodeData::Element { .. })
    }

    pub fn is_text_node(&self) -> bool {
        matches!(self.raw_dom_data, NodeData::Text { .. })
    }

    pub fn element_data(&self) -> Option<&ElementNodeData> {
        match self.raw_dom_data {
            NodeData::Element(ref data) => Some(data),
            NodeData::AnonymousBlock(ref data) => Some(data),
            _ => None,
        }
    }

    pub fn element_data_mut(&mut self) -> Option<&mut ElementNodeData> {
        match self.raw_dom_data {
            NodeData::Element(ref mut data) => Some(data),
            NodeData::AnonymousBlock(ref mut data) => Some(data),
            _ => None,
        }
    }

    pub fn text_data(&self) -> Option<&TextNodeData> {
        match self.raw_dom_data {
            NodeData::Text(ref data) => Some(data),
            _ => None,
        }
    }

    pub fn text_data_mut(&mut self) -> Option<&mut TextNodeData> {
        match self.raw_dom_data {
            NodeData::Text(ref mut data) => Some(data),
            _ => None,
        }
    }

    pub fn node_debug_str(&self) -> String {
        let mut s = String::new();

        match &self.raw_dom_data {
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
                if !class.is_empty() {
                    write!(
                        s,
                        "<{} class=\"{}\"> ({:?})",
                        name.local, class, self.display_outer
                    )
                } else {
                    write!(s, "<{}> ({:?})", name.local, self.display_outer)
                }
            } // NodeData::ProcessingInstruction { .. } => write!(s, "ProcessingInstruction"),
        }
        .unwrap();
        s
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
                |data: &Option<ElementData>| -> &ComputedValues {
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
        match &self.raw_dom_data {
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

    pub fn flush_style_attribute(&mut self) {
        if let NodeData::Element(ref mut elem_data) = self.raw_dom_data {
            elem_data.flush_style_attribute(&self.guard);
        }
    }

    pub fn order(&self) -> i32 {
        self.stylo_element_data
            .borrow()
            .as_ref()
            .and_then(|data| data.styles.get_primary())
            .map(|s| s.get_position().order)
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
        let x = x - self.final_layout.location.x;
        let y = y - self.final_layout.location.y;

        let size = self.final_layout.size;
        if x < 0.0 || x > size.width || y < 0.0 || y > size.height {
            return None;
        }

        // Call `.hit()` on each child in turn. If any return `Some` then return that value. Else return `Some(self.id).
        self.children
            .iter()
            .find_map(|&i| self.with(i).hit(x, y))
            .or(Some(HitResult {
                node_id: self.id,
                x,
                y,
            }))
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
            .field("is_inline_root", &self.is_inline_root)
            .field("child_idx", &self.child_idx)
            .field("children", &self.children)
            .field("layout_children", &self.layout_children.borrow())
            // .field("style", &self.style)
            .field("node", &self.raw_dom_data)
            .field("stylo_element_data", &self.stylo_element_data)
            // .field("unrounded_layout", &self.unrounded_layout)
            // .field("final_layout", &self.final_layout)
            .finish()
    }
}
