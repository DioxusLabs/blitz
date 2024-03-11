use std::cell::{Cell, RefCell};

use atomic_refcell::AtomicRefCell;
use html5ever::{local_name, tendril::StrTendril, Attribute, LocalName, QualName};
use markup5ever_rcdom::{Handle, NodeData};
use selectors::matching::QuirksMode;
use slab::Slab;
use std::fmt::Write;
use style::stylesheets::UrlExtraData;
use style::{
    data::ElementData,
    properties::{parse_style_attribute, PropertyDeclaration, PropertyDeclarationBlock},
    servo_arc::Arc,
    shared_lock::{Locked, SharedRwLock},
    stylesheets::CssRuleType,
    values::specified::Attr,
    Atom,
};
use style_traits::dom::ElementState;
use taffy::{
    prelude::{Layout, Style},
    Cache,
};
use url::Url;

// todo: might be faster to migrate this to ecs and split apart at a different boundary
pub struct Node {
    /// Our parent's ID
    pub parent: Option<usize>,

    /// Our Id
    pub id: usize,

    // Which child are we in our parent?
    pub child_idx: usize,

    // What are our children?
    // Might want to use a linkedlist or something better at precise inserts/delets
    pub children: Vec<usize>,

    // might want to make this weak
    // pub dom_data: DomData,
    pub node: Handle,

    // This little bundle of joy is our layout data from taffy and our style data from stylo
    //
    // todo: layout from new taffy
    pub data: AtomicRefCell<ElementData>,

    // need to make sure we sync this style and the other style...
    pub style: Style,

    pub cache: Cache,

    pub unrounded_layout: Layout,

    pub final_layout: Layout,

    // todo: this takes up a lot of space and should not be here if it doesn't have to be
    pub guard: SharedRwLock,

    pub flow: FlowType,

    pub additional_data: DomData,

    // The actual tree we belong to
    // this is unsafe!!
    pub tree: *mut Slab<Node>,
}

#[derive(Default)]
pub struct DomData {
    pub hidden: bool,
    pub style_attribute: Option<Arc<Locked<PropertyDeclarationBlock>>>,
}

#[derive(Debug, Clone, Copy)]
pub enum FlowType {
    Block,
    Flex,
    Grid,
    Inline,
    Table,
}

/*
-> Computed styles
-> Layout
-----> Needs to happen only when styles are computed
*/

type DomRefCell<T> = RefCell<T>;

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
        matches!(self.node.data, NodeData::Element { .. })
    }

    pub fn is_text_node(&self) -> bool {
        matches!(self.node.data, NodeData::Text { .. })
    }

    pub fn node_debug_str(&self) -> String {
        let mut s = String::new();

        fn get_attr(attrs: &Vec<Attribute>, name: LocalName) -> &str {
            attrs
                .iter()
                .find(|a| a.name.local == name)
                .map(|a| std::str::from_utf8(a.value.as_bytes()).unwrap_or("INVALID UTF8"))
                .unwrap_or("")
        }

        match &self.node.data {
            NodeData::Document => write!(s, "DOCUMENT"),
            NodeData::Doctype { name, .. } => write!(s, "DOCTYPE {name}"),
            NodeData::Text { contents } => {
                let contents = contents.borrow();
                let bytes = contents.as_bytes();
                write!(
                    s,
                    "TEXT {}",
                    &std::str::from_utf8(bytes.split_at(10.min(bytes.len())).0)
                        .unwrap_or("INVALID UTF8")
                )
            }
            NodeData::Comment { contents } => write!(
                s,
                "COMMENT {}",
                &std::str::from_utf8(contents.as_bytes().split_at(10).0).unwrap_or("INVALID UTF8")
            ),
            NodeData::Element { name, attrs, .. } => {
                let attrs = attrs.borrow();
                let klass = get_attr(&attrs, local_name!("class"));
                if klass.len() > 0 {
                    write!(s, "<{} class=\"{}\">", name.local, klass)
                } else {
                    write!(s, "<{}>", name.local)
                }
            }
            NodeData::ProcessingInstruction { .. } => write!(s, "ProcessingInstruction"),
        }
        .unwrap();
        s
    }

    pub fn attrs(&self) -> &RefCell<Vec<Attribute>> {
        match &self.node.data {
            NodeData::Element { attrs, .. } => attrs,
            _ => panic!("not an element"),
        }
    }

    pub fn flush_style_attribute(&mut self) {
        let arc = {
            let binding = self.attrs().borrow();
            let attr = binding
                .iter()
                .find(|attr| attr.name.local.as_ref() == "style");

            let Some(attr) = attr else {
                return;
            };

            let url = UrlExtraData::from(
                "data:text/css;charset=utf-8;base64,"
                    .parse::<Url>()
                    .unwrap(),
            );

            Arc::new(self.guard.wrap(parse_style_attribute(
                &attr.value,
                &url,
                None,
                QuirksMode::NoQuirks,
                CssRuleType::Style,
            )))
        };

        self.additional_data.style_attribute = Some(arc);
    }

    pub fn order(&self) -> i32 {
        self.data
            .borrow()
            .styles
            .get_primary()
            .map(|style| style.get_position().order)
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
        f.debug_struct("NodeData")
            .field("parent", &self.parent)
            .field("id", &self.id)
            .field("child_idx", &self.child_idx)
            .field("children", &self.children)
            // .field("style", &self.style)
            .field("node", &self.node)
            .field("data", &self.data)
            .field("unrounded_layout", &self.unrounded_layout)
            .field("final_layout", &self.final_layout)
            .finish()
    }
}
