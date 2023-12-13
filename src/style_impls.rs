use std::collections::HashMap;

use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};
use dioxus::prelude::LazyNodes;
use euclid::{Rect, Scale, Size2D};
use fxhash::FxHashMap;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, RcDom};
use selectors::{
    matching::{ElementSelectorFlags, MatchingContext, VisitedHandlingMode},
    sink::Push,
    OpaqueElement,
};
use servo_url::ServoUrl;
use slab::Slab;
use style::{
    context::{
        QuirksMode, RegisteredSpeculativePainter, RegisteredSpeculativePainters,
        SharedStyleContext, StyleContext,
    },
    data::ElementData,
    dom::{NodeInfo, OpaqueNode, TDocument, TElement, TNode, TShadowRoot},
    media_queries::MediaType,
    media_queries::{Device as StyleDevice, MediaList},
    properties::PropertyId,
    selector_parser::SelectorImpl,
    servo_arc::Arc,
    shared_lock::SharedRwLock,
    sharing::StyleSharingCandidate,
    stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet},
    stylist::Stylist,
    traversal::{DomTraversal, PerLevelTraversalData},
    Atom,
};
use style_traits::{dom::ElementState, SpeculativePainter};

pub struct RealDom {
    pub nodes: Slab<NodeData>,
    pub document: RcDom,
    pub lock: SharedRwLock,
    // documents: HashMap<ServoUrl, BlitzDocument>,
}

impl std::fmt::Debug for RealDom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RealDom")
            .field("nodes", &self.nodes)
            .field("lock", &self.lock)
            .finish()
    }
}

impl RealDom {
    pub fn from_dioxus(nodes: LazyNodes) -> Self {
        Self::new(dioxus_ssr::render_lazy(nodes))
    }

    pub fn root(&self) -> BlitzNode {
        BlitzNode { dom: self, id: 0 }
    }

    pub fn new(html: String) -> RealDom {
        // parse the html into a slab of node
        let mut nodes = Slab::new();

        // parse the html into a document
        let document = html5ever::parse_document(RcDom::default(), Default::default())
            .from_utf8()
            .read_from(&mut html.as_bytes())
            .unwrap();

        fill_slab_with_handles(&mut nodes, document.document.clone(), 0, None);

        RealDom {
            nodes,
            document,
            lock: SharedRwLock::new(),
        }
    }
}

// Assign IDs to the RcDom nodes by walking the tree and pushing them into the slab
// We just care that the root is 0, all else can be whatever
// Returns the node that just got inserted
fn fill_slab_with_handles(
    slab: &mut Slab<NodeData>,
    node: Handle,
    child_index: usize,
    parent: Option<usize>,
) -> usize {
    // todo: we want to skip filling comments/scripts/control, etc
    // Dioxus-rsx won't generate this however, so we're fine for now, but elements and text nodes are different

    // Reserve an entry
    let id = {
        let entry = slab.vacant_entry();
        let id = entry.key();
        entry.insert(NodeData {
            id,
            style: Default::default(),
            child_id: child_index,
            children: vec![],
            parsed: node.clone(),
            parent,
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
        .map(|(idx, child)| fill_slab_with_handles(slab, child.clone(), idx, Some(id)))
        .collect();

    id
}

#[derive(Debug)]
pub struct NodeData {
    // todo: layout
    pub style: AtomicRefCell<ElementData>,

    pub children: Vec<usize>,

    pub id: usize,

    pub child_id: usize,

    pub parent: Option<usize>,

    // might want to make this weak
    pub parsed: markup5ever_rcdom::Handle,
}

// Like, we do even need separate types for elements/nodes/documents?
#[derive(Debug, Clone, Copy)]
pub struct BlitzNode<'a> {
    pub dom: &'a RealDom,
    pub id: usize,
}

impl<'a> BlitzNode<'a> {
    fn me(&self) -> &NodeData {
        &self.dom.nodes[self.id]
    }

    fn next(&self) -> Option<Self> {
        let node = self.me();

        self.dom.nodes[node.parent?]
            .children
            .get(node.child_id + 1)
            .map(|id| BlitzNode {
                id: *id,
                dom: self.dom,
            })
    }

    fn prev(&self) -> Option<Self> {
        let node = self.me();

        if node.child_id == 0 {
            return None;
        }

        self.dom.nodes[node.parent?]
            .children
            .get(node.child_id - 1)
            .map(|id| BlitzNode {
                id: *id,
                dom: self.dom,
            })
    }

    // Get the nth node in the parents child list
    fn forward(&self, n: usize) -> Option<Self> {
        let node = self.me();

        self.dom.nodes[node.parent?]
            .children
            .get(node.child_id + n)
            .map(|id| BlitzNode {
                id: *id,
                dom: self.dom,
            })
    }

    fn backward(&self, n: usize) -> Option<Self> {
        let node = self.me();

        if node.child_id < n {
            return None;
        }

        self.dom.nodes[node.parent?]
            .children
            .get(node.child_id - n)
            .map(|id| BlitzNode {
                id: *id,
                dom: self.dom,
            })
    }

    fn parent(&self) -> Option<Self> {
        self.me().parent.map(|id| BlitzNode { id, dom: self.dom })
    }

    fn is_element(&self) -> bool {
        matches!(
            self.me().parsed.data,
            markup5ever_rcdom::NodeData::Element { .. }
        )
    }

    fn is_text_node(&self) -> bool {
        matches!(
            self.me().parsed.data,
            markup5ever_rcdom::NodeData::Text { .. }
        )
    }
}

impl PartialEq for BlitzNode<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for BlitzNode<'_> {}

impl<'a> TDocument for BlitzNode<'a> {
    type ConcreteNode = BlitzNode<'a>;

    fn as_node(&self) -> Self::ConcreteNode {
        self.clone()
    }

    fn is_html_document(&self) -> bool {
        true
    }

    fn quirks_mode(&self) -> QuirksMode {
        QuirksMode::NoQuirks
    }

    fn shared_lock(&self) -> &SharedRwLock {
        &self.dom.lock
    }
}

impl<'a> NodeInfo for BlitzNode<'a> {
    fn is_element(&self) -> bool {
        self.is_element()
    }

    fn is_text_node(&self) -> bool {
        self.is_text_node()
    }
}

impl<'a> TShadowRoot for BlitzNode<'a> {
    type ConcreteNode = BlitzNode<'a>;

    fn as_node(&self) -> Self::ConcreteNode {
        self.clone()
    }

    fn host(&self) -> <Self::ConcreteNode as TNode>::ConcreteElement {
        todo!("Shadow roots not implemented")
    }

    fn style_data<'b>(&self) -> Option<&'b style::stylist::CascadeData>
    where
        Self: 'b,
    {
        todo!("Shadow roots not implemented")
    }
}

// components/styleaapper.rs:
impl<'a> TNode for BlitzNode<'a> {
    type ConcreteElement = BlitzNode<'a>;
    type ConcreteDocument = BlitzNode<'a>;
    type ConcreteShadowRoot = BlitzNode<'a>;

    fn parent_node(&self) -> Option<Self> {
        self.dom.nodes[self.id]
            .parent
            .map(|id| BlitzNode { id, dom: self.dom })
    }

    fn first_child(&self) -> Option<Self> {
        self.dom.nodes[self.id]
            .children
            .first()
            .map(|id| BlitzNode {
                id: *id,
                dom: self.dom,
            })
    }

    fn last_child(&self) -> Option<Self> {
        self.dom.nodes[self.id].children.last().map(|id| BlitzNode {
            id: *id,
            dom: self.dom,
        })
    }

    fn prev_sibling(&self) -> Option<Self> {
        let node = &self.dom.nodes[self.id];

        if node.child_id == 0 {
            return None;
        }

        self.dom.nodes[node.parent?]
            .children
            .get(node.child_id - 1)
            .map(|id| BlitzNode {
                id: *id,
                dom: self.dom,
            })
    }

    fn next_sibling(&self) -> Option<Self> {
        let node = &self.dom.nodes[self.id];

        self.dom.nodes[node.parent?]
            .children
            .get(node.child_id + 1)
            .map(|id| BlitzNode {
                id: *id,
                dom: self.dom,
            })
    }

    fn owner_doc(&self) -> Self::ConcreteDocument {
        BlitzNode {
            dom: self.dom,
            id: 0,
        }
    }

    fn is_in_document(&self) -> bool {
        true
    }

    // I think this is the same as parent_node only in the cases when the direct parent is not a real element, forcing us
    // to travel upwards
    //
    // For the sake of this demo, we're just going to return the parent node ann
    fn traversal_parent(&self) -> Option<Self::ConcreteElement> {
        self.parent_node().and_then(|node| node.as_element())
    }

    fn opaque(&self) -> OpaqueNode {
        OpaqueNode(self.id)
    }

    fn debug_id(self) -> usize {
        self.id
    }

    fn as_element(&self) -> Option<Self::ConcreteElement> {
        match self.me().parsed.data {
            markup5ever_rcdom::NodeData::Element { .. } => Some(self.clone()),
            _ => None,
        }
    }

    fn as_document(&self) -> Option<Self::ConcreteDocument> {
        if self.id != 0 {
            return None;
        };

        Some(self.clone())
    }

    fn as_shadow_root(&self) -> Option<Self::ConcreteShadowRoot> {
        todo!("Shadow roots aren't real, yet")
    }
}

impl<'a> selectors::Element for BlitzNode<'a> {
    type Impl = SelectorImpl;

    // use the ptr of the rc as the id
    fn opaque(&self) -> selectors::OpaqueElement {
        OpaqueElement::new(self.dom.nodes[self.id].parsed.as_ref())
    }

    fn parent_element(&self) -> Option<Self> {
        self.parent_node()
    }

    fn parent_node_is_shadow_root(&self) -> bool {
        false
    }

    fn containing_shadow_host(&self) -> Option<Self> {
        None
    }

    fn is_pseudo_element(&self) -> bool {
        false
    }

    // These methods are implemented naively since we only threaded real nodes and not fake nodes
    // we should try and use `find` instead of this foward/backward stuff since its ugly and slow
    fn prev_sibling_element(&self) -> Option<Self> {
        let mut n = 1;
        while let Some(node) = self.backward(n) {
            if node.is_element() {
                return Some(node);
            }
            n += 1;
        }

        None
    }

    fn next_sibling_element(&self) -> Option<Self> {
        let mut n = 1;
        while let Some(node) = self.forward(n) {
            if node.is_element() {
                return Some(node);
            }
            n += 1;
        }

        None
    }

    fn first_element_child(&self) -> Option<Self> {
        let mut children = self.dom_children();

        while let Some(child) = children.next() {
            if child.is_element() {
                return Some(child);
            }
        }

        None
    }

    fn is_html_element_in_html_document(&self) -> bool {
        true
    }

    fn has_local_name(
        &self,
        local_name: &<Self::Impl as selectors::SelectorImpl>::BorrowedLocalName,
    ) -> bool {
        todo!()
    }

    fn has_namespace(
        &self,
        ns: &<Self::Impl as selectors::SelectorImpl>::BorrowedNamespaceUrl,
    ) -> bool {
        todo!()
    }

    fn is_same_type(&self, other: &Self) -> bool {
        todo!()
    }

    fn attr_matches(
        &self,
        ns: &selectors::attr::NamespaceConstraint<
            &<Self::Impl as selectors::SelectorImpl>::NamespaceUrl,
        >,
        local_name: &<Self::Impl as selectors::SelectorImpl>::LocalName,
        operation: &selectors::attr::AttrSelectorOperation<
            &<Self::Impl as selectors::SelectorImpl>::AttrValue,
        >,
    ) -> bool {
        todo!()
    }

    fn match_non_ts_pseudo_class(
        &self,
        pc: &<Self::Impl as selectors::SelectorImpl>::NonTSPseudoClass,
        context: &mut MatchingContext<Self::Impl>,
    ) -> bool {
        false
    }

    fn match_pseudo_element(
        &self,
        pe: &<Self::Impl as selectors::SelectorImpl>::PseudoElement,
        context: &mut MatchingContext<Self::Impl>,
    ) -> bool {
        false
    }

    fn apply_selector_flags(&self, flags: ElementSelectorFlags) {
        todo!()
    }

    fn is_link(&self) -> bool {
        todo!()
    }

    fn is_html_slot_element(&self) -> bool {
        todo!()
    }

    fn has_id(
        &self,
        id: &<Self::Impl as selectors::SelectorImpl>::Identifier,
        case_sensitivity: selectors::attr::CaseSensitivity,
    ) -> bool {
        todo!()
    }

    fn has_class(
        &self,
        name: &<Self::Impl as selectors::SelectorImpl>::Identifier,
        case_sensitivity: selectors::attr::CaseSensitivity,
    ) -> bool {
        todo!()
    }

    fn imported_part(
        &self,
        name: &<Self::Impl as selectors::SelectorImpl>::Identifier,
    ) -> Option<<Self::Impl as selectors::SelectorImpl>::Identifier> {
        todo!()
    }

    fn is_part(&self, name: &<Self::Impl as selectors::SelectorImpl>::Identifier) -> bool {
        todo!()
    }

    fn is_empty(&self) -> bool {
        todo!()
    }

    fn is_root(&self) -> bool {
        todo!()
    }
}

impl std::hash::Hash for BlitzNode<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_usize(self.id)
    }
}

impl<'a> TElement for BlitzNode<'a> {
    type ConcreteNode = BlitzNode<'a>;

    type TraversalChildrenIterator = Traverser<'a>;

    fn as_node(&self) -> Self::ConcreteNode {
        todo!()
    }

    fn traversal_children(&self) -> style::dom::LayoutIterator<Self::TraversalChildrenIterator> {
        todo!()
    }

    fn is_html_element(&self) -> bool {
        todo!()
    }

    fn is_mathml_element(&self) -> bool {
        todo!()
    }

    fn is_svg_element(&self) -> bool {
        todo!()
    }

    fn style_attribute(
        &self,
    ) -> Option<
        style::servo_arc::ArcBorrow<
            style::shared_lock::Locked<style::properties::PropertyDeclarationBlock>,
        >,
    > {
        todo!()
    }

    fn animation_rule(
        &self,
        _: &SharedStyleContext,
    ) -> Option<Arc<style::shared_lock::Locked<style::properties::PropertyDeclarationBlock>>> {
        todo!()
    }

    fn transition_rule(
        &self,
        context: &SharedStyleContext,
    ) -> Option<Arc<style::shared_lock::Locked<style::properties::PropertyDeclarationBlock>>> {
        todo!()
    }

    fn state(&self) -> ElementState {
        todo!()
    }

    fn has_part_attr(&self) -> bool {
        todo!()
    }

    fn exports_any_part(&self) -> bool {
        todo!()
    }

    fn id(&self) -> Option<&style::Atom> {
        todo!()
    }

    fn each_class<F>(&self, callback: F)
    where
        F: FnMut(&style::values::AtomIdent),
    {
        todo!()
    }

    fn each_attr_name<F>(&self, callback: F)
    where
        F: FnMut(&style::LocalName),
    {
        todo!()
    }

    fn has_dirty_descendants(&self) -> bool {
        todo!()
    }

    fn has_snapshot(&self) -> bool {
        // todo: We want to implement snapshots at some point
        false
    }

    fn handled_snapshot(&self) -> bool {
        false
    }

    unsafe fn set_handled_snapshot(&self) {
        todo!()
    }

    unsafe fn set_dirty_descendants(&self) {
        todo!()
    }

    unsafe fn unset_dirty_descendants(&self) {
        todo!()
    }

    fn store_children_to_process(&self, n: isize) {
        todo!()
    }

    fn did_process_child(&self) -> isize {
        todo!()
    }

    unsafe fn ensure_data(&self) -> AtomicRefMut<style::data::ElementData> {
        todo!()
    }

    unsafe fn clear_data(&self) {
        todo!()
    }

    fn has_data(&self) -> bool {
        todo!()
    }

    fn borrow_data(&self) -> Option<AtomicRef<style::data::ElementData>> {
        self.dom.nodes[self.id].style.try_borrow().ok()
    }

    fn mutate_data(&self) -> Option<AtomicRefMut<style::data::ElementData>> {
        self.dom.nodes[self.id].style.try_borrow_mut().ok()
    }

    fn skip_item_display_fixup(&self) -> bool {
        todo!()
    }

    fn may_have_animations(&self) -> bool {
        todo!()
    }

    fn has_animations(&self, context: &SharedStyleContext) -> bool {
        todo!()
    }

    fn has_css_animations(
        &self,
        context: &SharedStyleContext,
        pseudo_element: Option<style::selector_parser::PseudoElement>,
    ) -> bool {
        todo!()
    }

    fn has_css_transitions(
        &self,
        context: &SharedStyleContext,
        pseudo_element: Option<style::selector_parser::PseudoElement>,
    ) -> bool {
        todo!()
    }

    fn shadow_root(&self) -> Option<<Self::ConcreteNode as TNode>::ConcreteShadowRoot> {
        todo!()
    }

    fn containing_shadow(&self) -> Option<<Self::ConcreteNode as TNode>::ConcreteShadowRoot> {
        todo!()
    }

    fn lang_attr(&self) -> Option<style::selector_parser::AttrValue> {
        todo!()
    }

    fn match_element_lang(
        &self,
        override_lang: Option<Option<style::selector_parser::AttrValue>>,
        value: &style::selector_parser::Lang,
    ) -> bool {
        todo!()
    }

    fn is_html_document_body_element(&self) -> bool {
        todo!()
    }

    fn synthesize_presentational_hints_for_legacy_attributes<V>(
        &self,
        visited_handling: VisitedHandlingMode,
        hints: &mut V,
    ) where
        V: Push<style::applicable_declarations::ApplicableDeclarationBlock>,
    {
        todo!()
    }

    fn local_name(
        &self,
    ) -> &<style::selector_parser::SelectorImpl as selectors::parser::SelectorImpl>::BorrowedLocalName
    {
        todo!()
    }

    fn namespace(&self)
    -> &<style::selector_parser::SelectorImpl as selectors::parser::SelectorImpl>::BorrowedNamespaceUrl{
        todo!()
    }

    fn query_container_size(
        &self,
        display: &style::values::specified::Display,
    ) -> euclid::default::Size2D<Option<app_units::Au>> {
        todo!()
    }
}

pub struct Traverser<'a> {
    lock: &'a SharedRwLock,
}

impl<'a> Iterator for Traverser<'a> {
    type Item = BlitzNode<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        todo!()
    }
}

pub struct BlitzTraversal<'a> {
    cx: &'a SharedStyleContext<'a>,
}
impl<'a> BlitzTraversal<'a> {
    pub(crate) fn new(cx: &'a SharedStyleContext) -> Self {
        Self { cx }
    }
}

impl<'a, E: TElement> DomTraversal<E> for BlitzTraversal<'a> {
    fn process_preorder<F>(
        &self,
        data: &PerLevelTraversalData,
        context: &mut StyleContext<E>,
        node: E::ConcreteNode,
        note_child: F,
    ) where
        F: FnMut(E::ConcreteNode),
    {
        todo!()
    }

    fn process_postorder(&self, contect: &mut StyleContext<E>, node: E::ConcreteNode) {
        // nothing to do yet, i'm not even sure what we're supposed to do here
    }

    fn shared_context(&self) -> &SharedStyleContext {
        self.cx
    }
}

/// Handle custom painters like images for layouting
///
/// todo: actually implement this
pub struct RegisteredPaintersImpl;
impl RegisteredSpeculativePainters for RegisteredPaintersImpl {
    fn get(&self, name: &Atom) -> Option<&dyn RegisteredSpeculativePainter> {
        None
    }
}

#[test]
fn assert_size_of_equals() {
    use std::mem;

    // fn assert_layout<E>() {
    //     assert_eq!(
    //         mem::size_of::<SharingCache<E>>(),
    //         mem::size_of::<TypelessSharingCache>()
    //     );
    //     assert_eq!(
    //         mem::align_of::<SharingCache<E>>(),
    //         mem::align_of::<TypelessSharingCache>()
    //     );
    // }

    // let size = mem::size_of::<StyleSharingCandidate<BlitzNode>>();
    // dbg!(size);
}
