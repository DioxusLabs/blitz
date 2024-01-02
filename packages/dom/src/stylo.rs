//! Enable the dom to participate in styling by servo
//!

use crate::util::to_taffy_margin;
use crate::{
    node::Node,
    util::{to_taffy_border, to_taffy_padding},
};

use std::{
    borrow::{Borrow, Cow},
    cell::{Cell, RefCell},
    collections::HashMap,
};

use atomic_refcell::{AtomicRef, AtomicRefCell, AtomicRefMut};
use euclid::{Rect, Scale, Size2D};
use fxhash::FxHashMap;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::NodeData;
use selectors::{
    matching::{ElementSelectorFlags, MatchingContext, VisitedHandlingMode},
    sink::Push,
    OpaqueElement,
};
use servo_url::ServoUrl;
use slab::Slab;
use string_cache::{DefaultAtom, EmptyStaticAtomSet, StaticAtomSet};
use style::{
    animation::DocumentAnimationSet,
    context::{
        QuirksMode, RegisteredSpeculativePainter, RegisteredSpeculativePainters,
        SharedStyleContext, StyleContext,
    },
    data::ElementData,
    dom::{LayoutIterator, NodeInfo, OpaqueNode, TDocument, TElement, TNode, TShadowRoot},
    global_style_data::GLOBAL_STYLE_DATA,
    media_queries::MediaType,
    media_queries::{Device as StyleDevice, MediaList},
    properties::{
        style_structs::{Border, Box as BoxStyle, Margin, Padding, Position},
        PropertyDeclarationBlock, PropertyId, StyleBuilder,
    },
    selector_parser::SelectorImpl,
    servo_arc::{Arc, ArcBorrow},
    shared_lock::{Locked, SharedRwLock, StylesheetGuards},
    sharing::StyleSharingCandidate,
    stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet},
    stylist::Stylist,
    thread_state::ThreadState,
    traversal::{DomTraversal, PerLevelTraversalData},
    traversal_flags::TraversalFlags,
    values::{AtomIdent, GenericAtomIdent},
    Atom,
};
use style_traits::dom::ElementState;
use taffy::{prelude::Style, LengthPercentageAuto};

impl crate::document::Document {
    /// Walk the whole tree, converting styles to layout
    pub fn flush_styles_to_layout(&mut self, children: Vec<usize>) {
        // make a floating element
        for child in children {
            let children = {
                let node = self.nodes.get_mut(child).unwrap();
                let data = node.data.borrow();

                if let Some(style) = data.styles.get_primary() {
                    let margin = style.get_margin();
                    let padding = style.get_padding();
                    let border = style.get_border();
                    let Position {
                        top,
                        right,
                        bottom,
                        left,
                        z_index,
                        flex_direction,
                        flex_wrap,
                        justify_content,
                        align_content,
                        align_items,
                        flex_grow,
                        flex_shrink,
                        align_self,
                        order,
                        flex_basis,
                        width,
                        min_width,
                        max_width,
                        height,
                        min_height,
                        max_height,
                        box_sizing,
                        column_gap,
                        aspect_ratio,
                    } = style.get_position();

                    let BoxStyle {
                        _servo_top_layer,
                        _servo_overflow_clip_box,
                        display,
                        position,
                        float,
                        clear,
                        vertical_align,
                        overflow_x,
                        overflow_y,
                        transform,
                        rotate,
                        scale,
                        translate,
                        perspective,
                        perspective_origin,
                        backface_visibility,
                        transform_style,
                        transform_origin,
                        container_type,
                        container_name,
                        original_display,
                    }: &BoxStyle = style.get_box();

                    // todo: support grid

                    // dbg!(style.get_position());
                    // dbg!(style.get_box());
                    // dbg!(style.get_inhe());
                    // hmmmmmm, these seem wrong, coming from stylo
                    let mut display_ = match display.inside() {
                        style::values::specified::box_::DisplayInside::Flex => taffy::Display::Flex,
                        style::values::specified::box_::DisplayInside::None => taffy::Display::None,
                        _ => taffy::Display::Block,
                    };

                    // todo: support these
                    match display.outside() {
                        style::values::specified::box_::DisplayOutside::None => {
                            display_ = taffy::Display::None
                        }
                        style::values::specified::box_::DisplayOutside::Inline => {}
                        style::values::specified::box_::DisplayOutside::Block => {}
                        style::values::specified::box_::DisplayOutside::TableCaption => {}
                        style::values::specified::box_::DisplayOutside::InternalTable => {}
                    };

                    let align_content = match align_content {
                        style::computed_values::align_content::T::Stretch => {
                            Some(taffy::AlignContent::Stretch)
                        }
                        style::computed_values::align_content::T::FlexStart => {
                            Some(taffy::AlignContent::FlexStart)
                        }
                        style::computed_values::align_content::T::FlexEnd => {
                            Some(taffy::AlignContent::FlexEnd)
                        }
                        style::computed_values::align_content::T::Center => {
                            Some(taffy::AlignContent::Center)
                        }
                        style::computed_values::align_content::T::SpaceBetween => {
                            Some(taffy::AlignContent::SpaceBetween)
                        }
                        style::computed_values::align_content::T::SpaceAround => {
                            Some(taffy::AlignContent::SpaceAround)
                        }
                    };

                    let flex_direction = match flex_direction {
                        style::computed_values::flex_direction::T::Row => taffy::FlexDirection::Row,
                        style::computed_values::flex_direction::T::RowReverse => {
                            taffy::FlexDirection::RowReverse
                        }
                        style::computed_values::flex_direction::T::Column => {
                            taffy::FlexDirection::Column
                        }
                        style::computed_values::flex_direction::T::ColumnReverse => {
                            taffy::FlexDirection::ColumnReverse
                        }
                    };

                    let align_items = match align_items {
                        style::computed_values::align_items::T::Stretch => {
                            Some(taffy::AlignItems::Stretch)
                        }
                        style::computed_values::align_items::T::FlexStart => {
                            Some(taffy::AlignItems::FlexStart)
                        }
                        style::computed_values::align_items::T::FlexEnd => {
                            Some(taffy::AlignItems::FlexEnd)
                        }
                        style::computed_values::align_items::T::Center => {
                            Some(taffy::AlignItems::Center)
                        }
                        style::computed_values::align_items::T::Baseline => {
                            Some(taffy::AlignItems::Baseline)
                        }
                    };
                    node.style = Style {
                        margin: to_taffy_margin(margin),
                        padding: to_taffy_padding(padding),
                        border: to_taffy_border(border),
                        align_content,
                        display: display_,
                        flex_direction,
                        justify_content: match justify_content {
                            style::computed_values::justify_content::T::FlexStart => {
                                Some(taffy::JustifyContent::FlexStart)
                            }
                            style::computed_values::justify_content::T::Stretch => {
                                Some(taffy::JustifyContent::Stretch)
                            }
                            style::computed_values::justify_content::T::FlexEnd => {
                                Some(taffy::JustifyContent::FlexEnd)
                            }
                            style::computed_values::justify_content::T::Center => {
                                Some(taffy::JustifyContent::Center)
                            }
                            style::computed_values::justify_content::T::SpaceBetween => {
                                Some(taffy::JustifyContent::SpaceBetween)
                            }
                            style::computed_values::justify_content::T::SpaceAround => {
                                Some(taffy::JustifyContent::SpaceAround)
                            }
                        },
                        align_self: match align_self {
                            style::computed_values::align_self::T::Auto => align_items,
                            style::computed_values::align_self::T::Stretch => {
                                Some(taffy::AlignItems::Stretch)
                            }
                            style::computed_values::align_self::T::FlexStart => {
                                Some(taffy::AlignItems::FlexStart)
                            }
                            style::computed_values::align_self::T::FlexEnd => {
                                Some(taffy::AlignItems::FlexEnd)
                            }
                            style::computed_values::align_self::T::Center => {
                                Some(taffy::AlignItems::Center)
                            }
                            style::computed_values::align_self::T::Baseline => {
                                Some(taffy::AlignItems::Baseline)
                            }
                        },
                        flex_grow: flex_grow.0,
                        flex_shrink: flex_shrink.0,
                        align_items,
                        flex_wrap: match flex_wrap {
                            style::computed_values::flex_wrap::T::Wrap => taffy::FlexWrap::Wrap,
                            style::computed_values::flex_wrap::T::WrapReverse => {
                                taffy::FlexWrap::WrapReverse
                            }
                            style::computed_values::flex_wrap::T::Nowrap => taffy::FlexWrap::NoWrap,
                        },
                        flex_basis: match flex_basis {
                            style::values::generics::flex::FlexBasis::Content => {
                                taffy::Dimension::Auto
                            }
                            style::values::generics::flex::FlexBasis::Size(size) => match size {
                                style::values::generics::length::GenericSize::LengthPercentage(
                                    p,
                                ) => {
                                    if let Some(p) = p.0.to_percentage() {
                                        taffy::Dimension::Percent(p.0)
                                    } else {
                                        taffy::Dimension::Length(p.0.to_length().unwrap().px())
                                    }
                                }
                                style::values::generics::length::GenericSize::Auto => {
                                    taffy::Dimension::Auto
                                }
                            },
                        },
                        size: make_taffy_size(width, height),
                        // display
                        // overflow
                        // scrollbar_width
                        // position
                        // inset
                        // size
                        // min_size
                        // max_size
                        // aspect_ratio
                        // margin
                        // align_items
                        // align_self
                        // justify_items
                        // justify_self
                        // align_content
                        // justify_content
                        // gap
                        // flex_direction
                        // flex_wrap
                        // flex_basis
                        // flex_grow
                        // flex_shrink
                        // grid_template_rows
                        // grid_template_columns
                        // grid_auto_rows
                        // grid_auto_columns
                        // grid_auto_flow
                        // grid_row
                        // grid_column
                        ..Style::DEFAULT
                    };

                    // // now we need to override the style if there is a style attribute
                    // let style_attr = node
                    //     .attrs()
                    //     .borrow()
                    //     .iter()
                    //     .find(|attr| attr.name.local.as_ref() == "style");

                    // if let Some(style_attr) = style_attr {
                    //     // style::parser::ParserContext
                    // }
                }

                // would like to change this not require a clone, but requires some refactoring
                node.children.clone()
            };

            self.flush_styles_to_layout(children);
        }
    }

    pub fn resolve_stylist(&mut self) {
        style::thread_state::enter(ThreadState::LAYOUT);

        let guard = &self.guard;
        let guards = StylesheetGuards {
            author: &guard.read(),
            ua_or_user: &guard.read(),
        };

        let root = TDocument::as_node(&&self.nodes[0])
            .first_child()
            .unwrap()
            .as_element()
            .unwrap();

        self.stylist
            .flush(&guards, Some(root), Some(&self.snapshots));

        // Build the style context used by the style traversal
        let context = SharedStyleContext {
            traversal_flags: TraversalFlags::empty(),
            stylist: &self.stylist,
            options: GLOBAL_STYLE_DATA.options.clone(),
            guards,
            visited_styles_enabled: false,
            animations: (&DocumentAnimationSet::default()).clone(),
            current_time_for_animations: 0.0,
            snapshot_map: &self.snapshots,
            registered_speculative_painters: &RegisteredPaintersImpl,
        };

        // components/layout_2020/lib.rs:983
        let root = self.root_element();
        let token = RecalcStyle::pre_traverse(root, &context);

        if token.should_traverse() {
            // Style the elements, resolving their data
            let traverser = RecalcStyle::new(context);
            style::driver::traverse_dom(&traverser, token, None);
        }

        style::thread_state::exit(ThreadState::LAYOUT);
    }
}

fn make_taffy_size(
    width: &style::values::generics::length::GenericSize<
        style::values::generics::NonNegative<style::values::computed::LengthPercentage>,
    >,
    height: &style::values::generics::length::GenericSize<
        style::values::generics::NonNegative<style::values::computed::LengthPercentage>,
    >,
) -> taffy::prelude::Size<taffy::prelude::Dimension> {
    let width = match width {
        style::values::generics::length::GenericSize::LengthPercentage(p) => {
            if let Some(p) = p.0.to_percentage() {
                taffy::Dimension::Percent(p.0)
            } else {
                taffy::Dimension::Length(p.0.to_length().unwrap().px())
            }
        }
        style::values::generics::length::GenericSize::Auto => taffy::Dimension::Auto,
    };

    let height = match height {
        style::values::generics::length::GenericSize::LengthPercentage(p) => {
            if let Some(p) = p.0.to_percentage() {
                taffy::Dimension::Percent(p.0)
            } else {
                match &p.0.to_length() {
                    Some(p) => taffy::Dimension::Length(p.px()),

                    // todo: taffy needs to support calc
                    None => taffy::Dimension::Auto,
                }
            }
        }
        style::values::generics::length::GenericSize::Auto => taffy::Dimension::Auto,
    };

    taffy::prelude::Size { width, height }
}

/// A handle to a node that Servo's style traits are implemented against
///
/// Since BlitzNodes are not persistent (IE we don't keep the pointers around between frames), we choose to just implement
/// the tree structure in the nodes themselves, and temporarily give out pointers during the layout phase.
type BlitzNode<'a> = &'a Node;

impl<'a> TDocument for BlitzNode<'a> {
    type ConcreteNode = BlitzNode<'a>;

    fn as_node(&self) -> Self::ConcreteNode {
        self
    }

    fn is_html_document(&self) -> bool {
        true
    }

    fn quirks_mode(&self) -> QuirksMode {
        QuirksMode::NoQuirks
    }

    fn shared_lock(&self) -> &SharedRwLock {
        &self.guard
    }
}

impl<'a> NodeInfo for BlitzNode<'a> {
    fn is_element(&self) -> bool {
        Node::is_element(self)
    }

    fn is_text_node(&self) -> bool {
        Node::is_text_node(self)
    }
}

impl<'a> TShadowRoot for BlitzNode<'a> {
    type ConcreteNode = BlitzNode<'a>;

    fn as_node(&self) -> Self::ConcreteNode {
        self
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
        self.parent.map(|id| self.with(id))
    }

    fn first_child(&self) -> Option<Self> {
        self.children.first().map(|id| self.with(*id))
    }

    fn last_child(&self) -> Option<Self> {
        self.children.last().map(|id| self.with(*id))
    }

    fn prev_sibling(&self) -> Option<Self> {
        self.backward(1)
    }

    fn next_sibling(&self) -> Option<Self> {
        self.forward(1)
    }

    fn owner_doc(&self) -> Self::ConcreteDocument {
        self.with(0)
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
        OpaqueNode(self as *const _ as usize)
    }

    fn debug_id(self) -> usize {
        self.id
    }

    fn as_element(&self) -> Option<Self::ConcreteElement> {
        match self.node.data {
            NodeData::Element { .. } => Some(self),
            _ => None,
        }
    }

    fn as_document(&self) -> Option<Self::ConcreteDocument> {
        panic!();
        if self.id != 0 {
            return None;
        };

        Some(self)
    }

    fn as_shadow_root(&self) -> Option<Self::ConcreteShadowRoot> {
        todo!("Shadow roots aren't real, yet")
    }
}

impl<'a> selectors::Element for BlitzNode<'a> {
    type Impl = SelectorImpl;

    // use the ptr of the rc as the id
    fn opaque(&self) -> selectors::OpaqueElement {
        OpaqueElement::new(self)
    }

    fn parent_element(&self) -> Option<Self> {
        TElement::traversal_parent(&self)
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
        let data = self;
        match &data.node.data {
            NodeData::Element { name, .. } => &name.local == local_name,
            _ => false,
        }
    }

    fn has_namespace(
        &self,
        ns: &<Self::Impl as selectors::SelectorImpl>::BorrowedNamespaceUrl,
    ) -> bool {
        let data = self;
        match &data.node.data {
            NodeData::Element { name, .. } => &name.ns == ns,
            _ => false,
        }
    }

    fn is_same_type(&self, other: &Self) -> bool {
        false
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
        let mut has_attr = false;
        self.each_attr_name(|f| {
            if f.as_ref() == local_name.as_ref() {
                has_attr = true;
            }
        });
        has_attr
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
        // unimplemented!()
    }

    fn is_link(&self) -> bool {
        false
        // self.me()
        //     .parsed.data;
        // .borrow()
        // .iter()
        // .any(|(k, _)| k.local == "href")
    }

    fn is_html_slot_element(&self) -> bool {
        false
    }

    fn has_id(
        &self,
        id: &<Self::Impl as selectors::SelectorImpl>::Identifier,
        case_sensitivity: selectors::attr::CaseSensitivity,
    ) -> bool {
        let mut has_id = false;
        self.each_attr_name(|f| {
            if f.as_ref() == "id" {
                has_id = true;
            }
        });
        has_id
    }

    fn has_class(
        &self,
        search_name: &<Self::Impl as selectors::SelectorImpl>::Identifier,
        case_sensitivity: selectors::attr::CaseSensitivity,
    ) -> bool {
        let Some(al) = self.as_element() else {
            return false;
        };
        let data = al.node.data.borrow();
        let NodeData::Element { name, attrs, .. } = data else {
            return false;
        };
        let attrs = attrs.borrow();

        for attr in attrs.iter() {
            // make sure we only select class attributes
            if attr.name.local.as_ref() != "class" {
                continue;
            }

            // split the class attribute
            for pheme in attr.value.split_ascii_whitespace() {
                if pheme == search_name.as_ref() {
                    return true;
                }
            }
        }

        false
    }

    fn imported_part(
        &self,
        name: &<Self::Impl as selectors::SelectorImpl>::Identifier,
    ) -> Option<<Self::Impl as selectors::SelectorImpl>::Identifier> {
        None
    }

    fn is_part(&self, name: &<Self::Impl as selectors::SelectorImpl>::Identifier) -> bool {
        false
    }

    fn is_empty(&self) -> bool {
        self.dom_children().next().is_none()
    }

    fn is_root(&self) -> bool {
        self.parent_node().is_none()
    }
}

impl<'a> TElement for BlitzNode<'a> {
    type ConcreteNode = BlitzNode<'a>;

    type TraversalChildrenIterator = Traverser<'a>;

    fn as_node(&self) -> Self::ConcreteNode {
        self
    }

    fn traversal_children(&self) -> style::dom::LayoutIterator<Self::TraversalChildrenIterator> {
        LayoutIterator(Traverser {
            dom: self.tree(),
            parent: self,
            child_index: 0,
        })
    }

    fn is_html_element(&self) -> bool {
        self.is_element()
    }

    // not implemented.....
    fn is_mathml_element(&self) -> bool {
        false
    }

    // need to check the namespace, maybe?
    fn is_svg_element(&self) -> bool {
        false
    }

    fn style_attribute(&self) -> Option<ArcBorrow<Locked<PropertyDeclarationBlock>>> {
        self.additional_data
            .style_attribute
            .as_ref()
            .map(|f| f.borrow_arc())
    }

    fn animation_rule(
        &self,
        _: &SharedStyleContext,
    ) -> Option<Arc<Locked<PropertyDeclarationBlock>>> {
        None
    }

    fn transition_rule(
        &self,
        context: &SharedStyleContext,
    ) -> Option<Arc<Locked<PropertyDeclarationBlock>>> {
        None
    }

    fn state(&self) -> ElementState {
        // todo: we should track this
        ElementState::empty()
    }

    fn has_part_attr(&self) -> bool {
        false
    }

    fn exports_any_part(&self) -> bool {
        false
    }

    fn id(&self) -> Option<&style::Atom> {
        // None
        let attrs = match &self.node.data {
            NodeData::Element { ref attrs, .. } => attrs,
            _ => return None,
        };

        let attrs = attrs.borrow();

        let attr_id = attrs.iter().find(|id| id.name.local.as_ref() == "id")?;

        let id = attr_id.value.as_ref();
        let atom = Atom::from(id);
        let leadcked = &*Box::leak(Box::new(atom));

        Some(leadcked)
    }

    fn each_class<F>(&self, mut callback: F)
    where
        F: FnMut(&style::values::AtomIdent),
    {
        let Some(al) = self.as_element() else {
            return;
        };
        let data = &al.node.data;
        let NodeData::Element { name, attrs, .. } = data else {
            return;
        };
        let attrs = attrs.borrow();

        for attr in attrs.iter() {
            // make sure we only select class attributes
            if attr.name.local.as_ref() != "class" {
                continue;
            }

            // split the class attribute
            for pheme in attr.value.split_ascii_whitespace() {
                let atom = Atom::from(pheme); // interns the string
                callback(AtomIdent::cast(&atom));
            }
        }
    }

    fn each_attr_name<F>(&self, mut callback: F)
    where
        F: FnMut(&style::LocalName),
    {
        let Some(al) = self.as_element() else {
            return;
        };
        let data = &al.node.data;
        let NodeData::Element { name, attrs, .. } = data else {
            return;
        };
        let attrs = attrs.borrow();

        for attr in attrs.iter() {
            let b = GenericAtomIdent(attr.name.local.clone());
            callback(&b);
        }
    }

    fn has_dirty_descendants(&self) -> bool {
        false
    }

    fn has_snapshot(&self) -> bool {
        // todo: We want to implement snapshots at some point
        false
    }

    fn handled_snapshot(&self) -> bool {
        unimplemented!()
    }

    unsafe fn set_handled_snapshot(&self) {
        unimplemented!()
    }

    unsafe fn set_dirty_descendants(&self) {}

    unsafe fn unset_dirty_descendants(&self) {}

    fn store_children_to_process(&self, n: isize) {
        unimplemented!()
    }

    fn did_process_child(&self) -> isize {
        unimplemented!()
    }

    unsafe fn ensure_data(&self) -> AtomicRefMut<style::data::ElementData> {
        self.data.borrow_mut()
    }

    unsafe fn clear_data(&self) {
        // unimplemented!()
    }

    fn has_data(&self) -> bool {
        // true
        false
        // true // all nodes should have data
    }

    fn borrow_data(&self) -> Option<AtomicRef<style::data::ElementData>> {
        self.data.try_borrow().ok()
    }

    fn mutate_data(&self) -> Option<AtomicRefMut<style::data::ElementData>> {
        self.data.try_borrow_mut().ok()
    }

    fn skip_item_display_fixup(&self) -> bool {
        false
    }

    fn may_have_animations(&self) -> bool {
        false
    }

    fn has_animations(&self, context: &SharedStyleContext) -> bool {
        false
    }

    fn has_css_animations(
        &self,
        context: &SharedStyleContext,
        pseudo_element: Option<style::selector_parser::PseudoElement>,
    ) -> bool {
        false
    }

    fn has_css_transitions(
        &self,
        context: &SharedStyleContext,
        pseudo_element: Option<style::selector_parser::PseudoElement>,
    ) -> bool {
        false
    }

    fn shadow_root(&self) -> Option<<Self::ConcreteNode as TNode>::ConcreteShadowRoot> {
        None
    }

    fn containing_shadow(&self) -> Option<<Self::ConcreteNode as TNode>::ConcreteShadowRoot> {
        None
    }

    fn lang_attr(&self) -> Option<style::selector_parser::AttrValue> {
        None
    }

    fn match_element_lang(
        &self,
        override_lang: Option<Option<style::selector_parser::AttrValue>>,
        value: &style::selector_parser::Lang,
    ) -> bool {
        false
    }

    fn is_html_document_body_element(&self) -> bool {
        match self.node.data {
            NodeData::Document => true,
            _ => false,
        }
    }

    fn synthesize_presentational_hints_for_legacy_attributes<V>(
        &self,
        visited_handling: VisitedHandlingMode,
        hints: &mut V,
    ) where
        V: Push<style::applicable_declarations::ApplicableDeclarationBlock>,
    {
    }

    fn local_name(
        &self,
    ) -> &<style::selector_parser::SelectorImpl as selectors::parser::SelectorImpl>::BorrowedLocalName
    {
        let data = self;
        match &data.node.data {
            NodeData::Element { name, .. } => &name.local,
            g => panic!("Not an element {g:?}"),
        }
    }

        fn namespace(&self)
    -> &<style::selector_parser::SelectorImpl as selectors::parser::SelectorImpl>::BorrowedNamespaceUrl{
        let data = self;
        match &data.node.data {
            NodeData::Element { name, .. } => &name.ns,
            _ => panic!("Not an element"),
        }
    }

    fn query_container_size(
        &self,
        display: &style::values::specified::Display,
    ) -> euclid::default::Size2D<Option<app_units::Au>> {
        unimplemented!()
    }
}

pub struct Traverser<'a> {
    dom: &'a Slab<Node>,
    parent: BlitzNode<'a>,
    child_index: usize,
}

impl<'a> Iterator for Traverser<'a> {
    type Item = BlitzNode<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.parent.children.get(self.child_index)?;

        let node = self.parent.with(*node);

        self.child_index += 1;

        Some(node)
    }
}

impl std::hash::Hash for BlitzNode<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_usize(self.id)
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

use style::traversal::recalc_style_at;

pub struct RecalcStyle<'a> {
    context: SharedStyleContext<'a>,
}

impl<'a> RecalcStyle<'a> {
    pub fn new(context: SharedStyleContext<'a>) -> Self {
        RecalcStyle { context }
    }
}

#[allow(unsafe_code)]
impl<'a, 'dom, E> DomTraversal<E> for RecalcStyle<'a>
where
    E: TElement,
    E::ConcreteNode: 'dom,
{
    fn process_preorder<F: FnMut(E::ConcreteNode)>(
        &self,
        traversal_data: &PerLevelTraversalData,
        context: &mut StyleContext<E>,
        node: E::ConcreteNode,
        note_child: F,
    ) {
        // Don't process textnodees in this traversala
        if node.is_text_node() {
            return;
        }

        let el = node.as_element().unwrap();
        let mut data = el.mutate_data().unwrap();
        recalc_style_at(self, traversal_data, context, el, &mut data, note_child);

        // Gets set later on
        unsafe { el.unset_dirty_descendants() }
    }

    #[inline]
    fn needs_postorder_traversal() -> bool {
        false
    }

    fn process_postorder(&self, _style_context: &mut StyleContext<E>, _node: E::ConcreteNode) {
        panic!("this should never be called")
    }

    #[inline]
    fn shared_context(&self) -> &SharedStyleContext {
        &self.context
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

#[test]
fn parse_inline() {
    // let attrs = style::attr::AttrValue::from_serialized_tokenlist(
    //     r#"visibility: hidden; left: 1306.5px; top: 50px; display: none;"#.to_string(),
    // );

    // let val = CSSInlineStyleDeclaration();
}
