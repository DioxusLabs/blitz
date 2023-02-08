use dioxus_native_core::prelude::*;
use taffy::prelude::Size;
use vello::kurbo::{Point, Shape};

use crate::{
    render::{get_abs_pos, get_shape},
    RealDom,
};

pub(crate) fn get_hovered(
    dom: &RealDom,
    viewport_size: &Size<u32>,
    mouse_pos: Point,
) -> Option<NodeId> {
    let mut hovered: Option<NodeId> = None;
    dom.traverse_depth_first(|node| {
        if node.get::<MouseEffected>().unwrap().0
            && check_hovered(dom, node, viewport_size, mouse_pos)
        {
            let new_id = node.id();
            if let Some(id) = hovered {
                if node.height() > dom.get(id).unwrap().height() {
                    hovered = Some(new_id);
                }
            } else {
                hovered = Some(new_id);
            }
        }
    });
    hovered
}

pub(crate) fn check_hovered(
    dom: &RealDom,
    node: NodeRef,
    viewport_size: &Size<u32>,
    mouse_pos: Point,
) -> bool {
    get_shape(node, viewport_size, get_abs_pos(node, dom)).contains(mouse_pos)
}

#[derive(Debug, Default, PartialEq, Clone)]
pub(crate) struct MouseEffected(bool);

impl Pass for MouseEffected {
    type ChildDependencies = ();
    type ParentDependencies = ();
    type NodeDependencies = ();
    const NODE_MASK: NodeMaskBuilder<'static> = NodeMaskBuilder::new().with_listeners();

    fn pass<'a>(
        &mut self,
        node_view: NodeView,
        _: <Self::NodeDependencies as Dependancy>::ElementBorrowed<'a>,
        parent: Option<<Self::ParentDependencies as Dependancy>::ElementBorrowed<'a>>,
        _: Option<Vec<<Self::ChildDependencies as Dependancy>::ElementBorrowed<'a>>>,
        _: &SendAnyMap,
    ) -> bool {
        let new = Self(
            node_view
                .listeners()
                .into_iter()
                .flatten()
                .any(|event| MOUSE_EVENTS.binary_search(&event).is_ok()),
        );
        if *self != new {
            *self = new;
            true
        } else {
            false
        }
    }

    fn create<'a>(
        node_view: NodeView<()>,
        node: <Self::NodeDependencies as Dependancy>::ElementBorrowed<'a>,
        parent: Option<<Self::ParentDependencies as Dependancy>::ElementBorrowed<'a>>,
        children: Option<Vec<<Self::ChildDependencies as Dependancy>::ElementBorrowed<'a>>>,
        context: &SendAnyMap,
    ) -> Self {
        let mut myself = Self::default();
        myself.pass(node_view, node, parent, children, context);
        myself
    }
}

const MOUSE_EVENTS: &[&str] = &[
    "hover",
    "mouseleave",
    "mouseenter",
    "mouseclick",
    "mouseover",
];
