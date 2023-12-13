use std::collections::HashMap;

use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};
use dioxus::prelude::LazyNodes;
use euclid::{Rect, Scale, Size2D};
use fxhash::FxHashMap;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::RcDom;
use selectors::{matching::VisitedHandlingMode, sink::Push};
use servo_url::ServoUrl;
use slab::Slab;
use style::{
    context::{QuirksMode, RegisteredSpeculativePainter, RegisteredSpeculativePainters},
    data::ElementData,
    dom::{NodeInfo, OpaqueNode, TDocument, TElement, TNode, TShadowRoot},
    media_queries::MediaType,
    media_queries::{Device as StyleDevice, MediaList},
    properties::PropertyId,
    selector_parser::SelectorImpl,
    servo_arc::Arc,
    shared_lock::SharedRwLock,
    stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet},
    stylist::Stylist,
    traversal::DomTraversal,
    Atom,
};
use style_traits::{dom::ElementState, SpeculativePainter};

pub struct RealDom {
    nodes: Slab<NodeData>,
    document: RcDom,
    lock: SharedRwLock,
    // documents: HashMap<ServoUrl, BlitzDocument>,
}

impl RealDom {
    pub fn from_dioxus(nodes: LazyNodes) -> Self {
        Self::new(dioxus_ssr::render_lazy(nodes))
    }

    pub fn root(&self) -> BlitzDocument {
        BlitzDocument {
            lock: &self.lock,
            id: 0,
        }
    }

    pub fn new(html: String) -> RealDom {
        // parse the html into a slab of node
        let nodes = Slab::new();

        // parse the html into a document
        let document = html5ever::parse_document(RcDom::default(), Default::default())
            .from_utf8()
            .read_from(&mut html.as_bytes())
            .unwrap();

        let lock = SharedRwLock::new();

        RealDom {
            nodes,
            document,
            lock,
        }
    }
}

struct NodeData {
    // todo: layout
    style: AtomicRefCell<ElementData>,
}

#[derive(Clone, Copy)]
pub struct BlitzDocument<'a> {
    lock: &'a SharedRwLock,
    id: usize,
}

impl<'a> TDocument for BlitzDocument<'a> {
    type ConcreteNode = BlitzNode<'a>;

    fn as_node(&self) -> Self::ConcreteNode {
        BlitzNode {
            id: self.id,
            lock: self.lock,
        }
    }

    fn is_html_document(&self) -> bool {
        true
    }

    fn quirks_mode(&self) -> QuirksMode {
        QuirksMode::NoQuirks
    }

    fn shared_lock(&self) -> &SharedRwLock {
        self.lock
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BlitzNode<'a> {
    id: usize,
    lock: &'a SharedRwLock,
}

impl PartialEq for BlitzNode<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<'a> NodeInfo for BlitzNode<'a> {
    fn is_element(&self) -> bool {
        todo!()
    }

    fn is_text_node(&self) -> bool {
        todo!()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BlitzShadowRoot<'a> {
    lock: &'a SharedRwLock,
}

impl PartialEq for BlitzShadowRoot<'_> {
    fn eq(&self, other: &Self) -> bool {
        todo!()
    }
}

impl<'a> TShadowRoot for BlitzShadowRoot<'a> {
    type ConcreteNode = BlitzNode<'a>;

    fn as_node(&self) -> Self::ConcreteNode {
        todo!()
    }

    fn host(&self) -> <Self::ConcreteNode as TNode>::ConcreteElement {
        todo!()
    }

    fn style_data<'b>(&self) -> Option<&'b style::stylist::CascadeData>
    where
        Self: 'b,
    {
        todo!()
    }
}

// components/styleaapper.rs:
impl<'a> TNode for BlitzNode<'a> {
    type ConcreteElement = BlitzElement<'a>;

    type ConcreteDocument = BlitzDocument<'a>;

    type ConcreteShadowRoot = BlitzShadowRoot<'a>;

    fn parent_node(&self) -> Option<Self> {
        todo!()
    }

    fn first_child(&self) -> Option<Self> {
        todo!()
    }

    fn last_child(&self) -> Option<Self> {
        todo!()
    }

    fn prev_sibling(&self) -> Option<Self> {
        todo!()
    }

    fn next_sibling(&self) -> Option<Self> {
        todo!()
    }

    fn owner_doc(&self) -> Self::ConcreteDocument {
        todo!()
    }

    fn is_in_document(&self) -> bool {
        todo!()
    }

    fn traversal_parent(&self) -> Option<Self::ConcreteElement> {
        todo!()
    }

    fn opaque(&self) -> OpaqueNode {
        OpaqueNode(self.id)
    }

    fn debug_id(self) -> usize {
        self.id
    }

    fn as_element(&self) -> Option<Self::ConcreteElement> {
        todo!()
    }

    fn as_document(&self) -> Option<Self::ConcreteDocument> {
        todo!()
    }

    fn as_shadow_root(&self) -> Option<Self::ConcreteShadowRoot> {
        todo!()
    }
}

impl<'a> selectors::Element for BlitzNode<'a> {
    type Impl = SelectorImpl;

    fn opaque(&self) -> selectors::OpaqueElement {
        todo!()
    }

    fn parent_element(&self) -> Option<Self> {
        todo!()
    }

    fn parent_node_is_shadow_root(&self) -> bool {
        todo!()
    }

    fn containing_shadow_host(&self) -> Option<Self> {
        todo!()
    }

    fn is_pseudo_element(&self) -> bool {
        todo!()
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        todo!()
    }

    fn next_sibling_element(&self) -> Option<Self> {
        todo!()
    }

    fn first_element_child(&self) -> Option<Self> {
        todo!()
    }

    fn is_html_element_in_html_document(&self) -> bool {
        todo!()
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
        context: &mut selectors::matching::MatchingContext<Self::Impl>,
    ) -> bool {
        todo!()
    }

    fn match_pseudo_element(
        &self,
        pe: &<Self::Impl as selectors::SelectorImpl>::PseudoElement,
        context: &mut selectors::matching::MatchingContext<Self::Impl>,
    ) -> bool {
        todo!()
    }

    fn apply_selector_flags(&self, flags: selectors::matching::ElementSelectorFlags) {
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

#[derive(Clone, Copy, Debug)]
pub struct BlitzElement<'a> {
    lock: &'a SharedRwLock,
}

impl Eq for BlitzElement<'_> {}
impl PartialEq for BlitzElement<'_> {
    fn eq(&self, other: &Self) -> bool {
        todo!()
    }
}

impl std::hash::Hash for BlitzElement<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        todo!()
    }
}

impl<'a> selectors::Element for BlitzElement<'a> {
    type Impl = SelectorImpl;

    fn opaque(&self) -> selectors::OpaqueElement {
        todo!()
    }

    fn parent_element(&self) -> Option<Self> {
        todo!()
    }

    fn parent_node_is_shadow_root(&self) -> bool {
        todo!()
    }

    fn containing_shadow_host(&self) -> Option<Self> {
        todo!()
    }

    fn is_pseudo_element(&self) -> bool {
        todo!()
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        todo!()
    }

    fn next_sibling_element(&self) -> Option<Self> {
        todo!()
    }

    fn first_element_child(&self) -> Option<Self> {
        todo!()
    }

    fn is_html_element_in_html_document(&self) -> bool {
        todo!()
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
        context: &mut selectors::matching::MatchingContext<Self::Impl>,
    ) -> bool {
        todo!()
    }

    fn match_pseudo_element(
        &self,
        pe: &<Self::Impl as selectors::SelectorImpl>::PseudoElement,
        context: &mut selectors::matching::MatchingContext<Self::Impl>,
    ) -> bool {
        todo!()
    }

    fn apply_selector_flags(&self, flags: selectors::matching::ElementSelectorFlags) {
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

impl<'a> TElement for BlitzElement<'a> {
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
        _: &style::context::SharedStyleContext,
    ) -> Option<Arc<style::shared_lock::Locked<style::properties::PropertyDeclarationBlock>>> {
        todo!()
    }

    fn transition_rule(
        &self,
        context: &style::context::SharedStyleContext,
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
        todo!()
    }

    fn handled_snapshot(&self) -> bool {
        todo!()
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
        todo!()
    }

    fn mutate_data(&self) -> Option<AtomicRefMut<style::data::ElementData>> {
        todo!()
    }

    fn skip_item_display_fixup(&self) -> bool {
        todo!()
    }

    fn may_have_animations(&self) -> bool {
        todo!()
    }

    fn has_animations(&self, context: &style::context::SharedStyleContext) -> bool {
        todo!()
    }

    fn has_css_animations(
        &self,
        context: &style::context::SharedStyleContext,
        pseudo_element: Option<style::selector_parser::PseudoElement>,
    ) -> bool {
        todo!()
    }

    fn has_css_transitions(
        &self,
        context: &style::context::SharedStyleContext,
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

pub struct BlitzTraversal {}
impl BlitzTraversal {
    pub(crate) fn new() -> Self {
        Self {}
    }
}

impl<E: TElement> DomTraversal<E> for BlitzTraversal {
    fn process_preorder<F>(
        &self,
        data: &style::traversal::PerLevelTraversalData,
        context: &mut style::context::StyleContext<E>,
        node: E::ConcreteNode,
        note_child: F,
    ) where
        F: FnMut(E::ConcreteNode),
    {
        todo!()
    }

    fn process_postorder(
        &self,
        contect: &mut style::context::StyleContext<E>,
        node: E::ConcreteNode,
    ) {
        todo!()
    }

    fn shared_context(&self) -> &style::context::SharedStyleContext {
        todo!()
    }
}

// pub struct RegisteredPainterImpl {
//     painter: Box<dyn Painter>,
//     name: Atom,
//     // FIXME: Should be a PrecomputedHashMap.
//     properties: FxHashMap<Atom, PropertyId>,
// }

// impl SpeculativePainter for RegisteredPainterImpl {
//     fn speculatively_draw_a_paint_image(
//         &self,
//         properties: Vec<(Atom, String)>,
//         arguments: Vec<String>,
//     ) {
//         self.painter
//             .speculatively_draw_a_paint_image(properties, arguments);
//     }
// }

// impl RegisteredSpeculativePainter for RegisteredPainterImpl {
//     fn properties(&self) -> &FxHashMap<Atom, PropertyId> {
//         &self.properties
//     }
//     fn name(&self) -> Atom {
//         self.name.clone()
//     }
// }

// impl Painter for RegisteredPainterImpl {
//     fn draw_a_paint_image(
//         &self,
//         size: Size2D<f32, CSSPixel>,
//         device_pixel_ratio: Scale<f32, CSSPixel, DevicePixel>,
//         properties: Vec<(Atom, String)>,
//         arguments: Vec<String>,
//     ) -> Result<DrawAPaintImageResult, PaintWorkletError> {
//         self.painter
//             .draw_a_paint_image(size, device_pixel_ratio, properties, arguments)
//     }
// }

pub struct RegisteredPaintersImpl;
// struct RegisteredPaintersImpl(FxHashMap<Atom, RegisteredPainterImpl>);

impl RegisteredSpeculativePainters for RegisteredPaintersImpl {
    fn get(&self, name: &Atom) -> Option<&dyn RegisteredSpeculativePainter> {
        None
        // self.0
        //     .get(&name)
        // .map(|painter| painter as &dyn RegisteredSpeculativePainter)
    }
}
