use atomic_refcell::{AtomicRef, AtomicRefCell};
use html5ever::{local_name, ns, LocalName, QualName};
use image::DynamicImage;
use selectors::matching::QuirksMode;
use slab::Slab;
use std::cell::RefCell;
use std::fmt::Write;
use std::mem::Discriminant;
use std::sync::Arc;
use style::values::computed::Display;
// use string_cache::Atom;
use std::borrow::Cow;
use style::properties::ComputedValues;
use style::stylesheets::UrlExtraData;
use style::Atom;
use style::{
    data::ElementData,
    properties::{parse_style_attribute, PropertyDeclarationBlock},
    servo_arc::Arc as ServoArc,
    shared_lock::{Locked, SharedRwLock},
    stylesheets::CssRuleType,
    values::specified::box_::DisplayInside,
    values::specified::box_::DisplayOutside,
};
use taffy::{
    prelude::{Layout, Style},
    Cache,
};
use url::Url;

use crate::events::EventListener;
use crate::layout::collect_layout_children;
use crate::Document;

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

    // Taffy layout data:
    pub style: Style,
    pub hidden: bool,
    pub display_outer: DisplayOuter,
    pub cache: Cache,
    pub unrounded_layout: Layout,
    pub final_layout: Layout,
    pub listeners: Vec<EventListener>,
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

            style: Default::default(),
            hidden: false,
            display_outer: DisplayOuter::Block,
            cache: Cache::new(),
            unrounded_layout: Layout::new(),
            final_layout: Layout::new(),
            listeners: Default::default(),
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
}

#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, PartialEq)]
enum LayoutChildKind {
    Regular,
    Anonymous,
}

#[derive(Debug, Clone)]
struct LayoutChild {
    kind: LayoutChildKind,
    node_id: usize,
}

#[derive(Debug, Clone)]
enum LayoutChildrenState {
    Uninit,
    Resolved(Vec<LayoutChild>),
    SameAsRegularChildren(LayoutChildKind),
    NoChildren,
}

// impl LayoutChildrenState {
//     fn from_dom(doc: &Document, container_node_id: usize) -> Self {}
// }

#[derive(Debug, Clone)]
pub struct ElementNodeData {
    /// The elements tag name, namespace and prefix
    pub name: QualName,

    /// The elements id attribute parsed as an atom (if it has one)
    pub id: Option<Atom>,

    /// The element's attributes
    pub attrs: Vec<Attribute>,

    /// The element's parsed style attribute (used by stylo)
    pub style_attribute: Option<ServoArc<Locked<PropertyDeclarationBlock>>>,

    /// Parley text layout (elements with inline inner display mode only)
    pub inline_layout: Option<Box<TextLayout>>,

    /// The element's image content (\<img\> element's only)
    pub image: Option<Arc<DynamicImage>>,

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
        ElementNodeData {
            name,
            id: id_attr_atom,
            attrs,
            style_attribute: Default::default(),
            inline_layout: None,
            image: None,
            template_contents: None,
            // listeners: FxHashSet::default(),
        }
    }

    pub fn attrs(&self) -> &[Attribute] {
        &self.attrs
    }

    pub fn attr(&self, name: impl PartialEq<LocalName>) -> Option<&str> {
        let attr = self.attrs.iter().find(|attr| name == attr.name.local)?;
        Some(&attr.value)
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
pub struct TextBrush;

#[derive(Clone)]
pub struct TextLayout {
    layout: parley::layout::Layout<TextBrush>,
}

impl std::fmt::Debug for TextLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TextLayout")
    }
}

impl TextLayout {
    pub fn new(
        font_ctx: &mut parley::FontContext,
        layout_ctx: &mut parley::LayoutContext<TextBrush>,
        text: &str,
        display_scale: f32,
        // styles: &ComputedValues,
    ) -> TextLayout {
        let mut builder = layout_ctx.ranged_builder(font_ctx, text, display_scale);

        // TODO: Apply styles

        Self {
            layout: builder.build(),
        }
    }

    pub fn measure(&mut self, max_width: Option<f32>) -> taffy::Size<f32> {
        self.layout
            .break_all_lines(max_width, parley::layout::Alignment::Start);
        self.measured_size()
    }

    pub fn measured_size(&self) -> taffy::Size<f32> {
        taffy::Size {
            width: self.layout.width(),
            height: self.layout.height(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TextNodeData {
    /// The textual content of the text node
    pub content: String,

    /// Parley text layout. Note that not all text nodes will have their own layout. Text nodes
    /// that are part of a larger inline context will be added to a layout higher up the tree
    pub layout: Option<Box<TextLayout>>,
}

impl TextNodeData {
    pub fn get_or_init_layout(
        &mut self,
        font_ctx: &mut parley::FontContext,
        layout_ctx: &mut parley::LayoutContext<TextBrush>,
        display_scale: f32,
        // styles: &ComputedValues,
    ) -> &mut TextLayout {
        let layout = TextLayout::new(font_ctx, layout_ctx, &self.content, display_scale);
        self.layout = Some(Box::new(layout));

        return &mut **self.layout.as_mut().unwrap();
    }
}

impl TextNodeData {
    pub fn new(content: String) -> Self {
        Self {
            content,
            layout: None,
        }
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
            self.node_debug_str().replace("\n", ""),
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
            _ => None,
        }
    }

    pub fn element_data_mut(&mut self) -> Option<&mut ElementNodeData> {
        match self.raw_dom_data {
            NodeData::Element(ref mut data) => Some(data),
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
                if class.len() > 0 {
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

    pub fn primary_styles<'c>(&'c self) -> Option<AtomicRef<'c, ComputedValues>> {
        let stylo_element_data = self.stylo_element_data.borrow();
        if stylo_element_data
            .as_ref()
            .and_then(|d| d.styles.get_primary())
            .is_some()
        {
            Some(AtomicRef::map(
                stylo_element_data,
                |data: &Option<ElementData>| -> &ComputedValues {
                    &**data.as_ref().unwrap().styles.get_primary().unwrap()
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
            NodeData::Element { .. } => {
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
    ///    - Some(self.id) is the position is within the node but doesn't match any children
    ///    - The result of recursively calling child.hit() on the the child element that is
    ///      positioned at that position if there is one.
    ///
    /// TODO: z-index
    /// (If multiple children are positioned at the position then a random one will be recursed into)
    pub fn hit(&self, x: f32, y: f32) -> Option<usize> {
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
            .or(Some(self.id))
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
            .field("child_idx", &self.child_idx)
            .field("children", &self.children)
            // .field("style", &self.style)
            .field("node", &self.raw_dom_data)
            .field("stylo_element_data", &self.stylo_element_data)
            .field("unrounded_layout", &self.unrounded_layout)
            .field("final_layout", &self.final_layout)
            .finish()
    }
}
