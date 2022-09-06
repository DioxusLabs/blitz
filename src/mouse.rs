use dioxus::core::ElementId;
use dioxus_native_core::{
    node_ref::{NodeMask, NodeView},
    state::NodeDepState,
};
use dioxus_native_core_macro::sorted_str_slice;
use piet_wgpu::kurbo::{Point, Shape};
use taffy::prelude::Size;

use crate::{
    render::{get_abs_pos, get_shape},
    Dom, DomNode,
};

pub(crate) fn get_hovered(
    dom: &Dom,
    viewport_size: &Size<u32>,
    mouse_pos: Point,
) -> Option<ElementId> {
    let mut hovered: Option<ElementId> = None;
    dom.traverse_depth_first(|node| {
        if node.state.mouse_effected.0 && check_hovered(dom, node, viewport_size, mouse_pos) {
            if let Some(id) = hovered {
                if node.height > dom[id].height {
                    hovered = Some(node.id);
                }
            } else {
                hovered = Some(node.id);
            }
        }
    });
    hovered
}

pub(crate) fn check_hovered(
    dom: &Dom,
    node: &DomNode,
    viewport_size: &Size<u32>,
    mouse_pos: Point,
) -> bool {
    get_shape(node, viewport_size, get_abs_pos(node, dom)).contains(mouse_pos)
}

#[derive(Debug, Default, PartialEq, Clone)]
pub(crate) struct MouseEffected(bool);

impl NodeDepState<()> for MouseEffected {
    type Ctx = ();

    const NODE_MASK: NodeMask = NodeMask::new().with_listeners();

    fn reduce(&mut self, node: NodeView<'_>, _sibling: (), _: &Self::Ctx) -> bool {
        let new = Self(
            node.listeners()
                .iter()
                .any(|l| MOUSE_EVENTS.binary_search(&l.event).is_ok()),
        );
        if *self != new {
            *self = new;
            true
        } else {
            false
        }
    }
}

const MOUSE_EVENTS: &[&str] = &sorted_str_slice!([
    "hover",
    "mouseleave",
    "mouseenter",
    "mouseclick",
    "mouseover"
]);
