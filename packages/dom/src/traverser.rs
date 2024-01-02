use style::context::{SharedStyleContext, StyleContext};
use style::dom::{NodeInfo, TElement, TNode};
use style::traversal::{recalc_style_at, DomTraversal, PerLevelTraversalData};

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
