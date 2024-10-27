//! Enable the dom to participate in styling by servo
//!

use super::stylo_to_taffy;
use crate::events::EventData;
use crate::events::HitResult;
use crate::handle::RecalcStyle;
use crate::handle::RegisteredPaintersImpl;
use crate::node::Node;
use crate::node::NodeData;
use crate::Handle;
use atomic_refcell::{AtomicRef, AtomicRefMut};
use html5ever::LocalNameStaticSet;
use html5ever::NamespaceStaticSet;
use html5ever::{local_name, LocalName, Namespace};
use selectors::{
    attr::{AttrSelectorOperation, AttrSelectorOperator, NamespaceConstraint},
    matching::{ElementSelectorFlags, MatchingContext, VisitedHandlingMode},
    sink::Push,
    Element, OpaqueElement,
};
use slab::Slab;
use std::sync::atomic::Ordering;
use style::applicable_declarations::ApplicableDeclarationBlock;
use style::color::AbsoluteColor;
use style::invalidation::element::restyle_hints::RestyleHint;
use style::properties::{Importance, PropertyDeclaration};
use style::rule_tree::CascadeLevel;
use style::selector_parser::PseudoElement;
use style::stylesheets::layer_rule::LayerOrder;
use style::values::computed::text::TextAlign as StyloTextAlign;
use style::values::computed::Percentage;
use style::values::specified::box_::DisplayOutside;
use style::values::AtomString;
use style::CaseSensitivityExt;
use style::{
    animation::DocumentAnimationSet,
    context::{
        QuirksMode, RegisteredSpeculativePainter, RegisteredSpeculativePainters,
        SharedStyleContext, StyleContext,
    },
    dom::{LayoutIterator, NodeInfo, OpaqueNode, TDocument, TElement, TNode, TShadowRoot},
    global_style_data::GLOBAL_STYLE_DATA,
    properties::PropertyDeclarationBlock,
    selector_parser::{NonTSPseudoClass, SelectorImpl},
    servo_arc::{Arc, ArcBorrow},
    shared_lock::{Locked, SharedRwLock, StylesheetGuards},
    thread_state::ThreadState,
    traversal::{DomTraversal, PerLevelTraversalData},
    traversal_flags::TraversalFlags,
    values::{AtomIdent, GenericAtomIdent},
    Atom,
};
use style_dom::ElementState;
use winit::event::Modifiers;

impl crate::document::Document {
    /// Walk the whole tree, converting styles to layout
    pub fn flush_styles_to_layout(&mut self, node_id: usize) {
        let display = {
            let node = self.nodes.get_mut(node_id).unwrap();
            let stylo_element_data = node.stylo_element_data.borrow();
            let primary_styles = stylo_element_data
                .as_ref()
                .and_then(|data| data.styles.get_primary());

            let Some(style) = primary_styles else {
                return;
            };

            node.style = stylo_to_taffy::entire_style(style);

            node.display_outer = match style.clone_display().outside() {
                DisplayOutside::None => crate::node::DisplayOuter::None,
                DisplayOutside::Inline => crate::node::DisplayOuter::Inline,
                DisplayOutside::Block => crate::node::DisplayOuter::Block,
                DisplayOutside::TableCaption => crate::node::DisplayOuter::Block,
                DisplayOutside::InternalTable => crate::node::DisplayOuter::Block,
            };

            // Clear Taffy cache
            // TODO: smarter cache invalidation
            node.cache.clear();

            node.style.display
        };

        // If the node has children, then take those children and...
        let children = self.nodes[node_id].layout_children.borrow_mut().take();
        if let Some(mut children) = children {
            // Recursively call flush_styles_to_layout on each child
            for child in children.iter() {
                self.flush_styles_to_layout(*child);
            }

            // If the node is a Flexbox or Grid node then sort by css order property
            if matches!(display, taffy::Display::Flex | taffy::Display::Grid) {
                children.sort_by(|left, right| {
                    let left_node = self.nodes.get(*left).unwrap();
                    let right_node = self.nodes.get(*right).unwrap();
                    left_node.order().cmp(&right_node.order())
                });
            }

            // Put children back
            *self.nodes[node_id].layout_children.borrow_mut() = Some(children);
        }
    }

    pub fn resolve_stylist(&mut self) {
        style::thread_state::enter(ThreadState::LAYOUT);

        let guard = &self.guard;
        let guards = StylesheetGuards {
            author: &guard.read(),
            ua_or_user: &guard.read(),
        };

        let root = TDocument::as_node(&Handle {
            node: &self.nodes[0],
            tree: &self.nodes,
        })
        .first_element_child()
        .unwrap()
        .as_element()
        .unwrap();

        // Force restyle all nodes
        // TODO: finer grained style invalidation
        let mut stylo_element_data = root.node.stylo_element_data.borrow_mut();
        if let Some(data) = &mut *stylo_element_data {
            data.hint |= RestyleHint::restyle_subtree();
            data.hint |= RestyleHint::recascade_subtree();
        }
        drop(stylo_element_data);

        self.stylist
            .flush(&guards, Some(root), Some(&self.snapshots));

        // Build the style context used by the style traversal
        let context = SharedStyleContext {
            traversal_flags: TraversalFlags::empty(),
            stylist: &self.stylist,
            options: GLOBAL_STYLE_DATA.options.clone(),
            guards,
            visited_styles_enabled: false,
            animations: DocumentAnimationSet::default().clone(),
            current_time_for_animations: 0.0,
            snapshot_map: &self.snapshots,
            registered_speculative_painters: &RegisteredPaintersImpl,
        };

        // components/layout_2020/lib.rs:983
        let root = self.root_element();
        // dbg!(root);
        let token = RecalcStyle::pre_traverse(
            Handle {
                node: root,
                tree: self.tree(),
            },
            &context,
        );

        if token.should_traverse() {
            // Style the elements, resolving their data
            let traverser = RecalcStyle::new(context);
            style::driver::traverse_dom(&traverser, token, None);
        }

        style::thread_state::exit(ThreadState::LAYOUT);
    }
}
