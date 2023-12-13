use style::context::{SharedStyleContext, StyleContext};
use style::data::ElementData;
use style::dom::{NodeInfo, TElement, TNode};
use style::traversal::{recalc_style_at, DomTraversal, PerLevelTraversalData};

pub struct RecalcStyle<'a> {
    context: SharedStyleContext<'a>,
}

impl<'a> RecalcStyle<'a> {
    pub fn new(context: SharedStyleContext<'a>) -> Self {
        RecalcStyle { context: context }
    }

    pub fn context(&self) -> &SharedStyleContext<'a> {
        &self.context
    }

    pub fn destroy(self) -> SharedStyleContext<'a> {
        self.context
    }
}

#[allow(unsafe_code)]
impl<'a, 'dom, E> DomTraversal<E> for RecalcStyle<'a>
where
    E: TElement,
    E::ConcreteNode: 'dom,
{
    fn process_preorder<F>(
        &self,
        traversal_data: &PerLevelTraversalData,
        context: &mut StyleContext<E>,
        node: E::ConcreteNode,
        note_child: F,
    ) where
        F: FnMut(E::ConcreteNode),
    {
        unsafe {
            // node.initialize_data();
            if !node.is_text_node() {
                let el = node.as_element().unwrap();
                let mut data = el.mutate_data().unwrap();
                recalc_style_at(self, traversal_data, context, el, &mut data, note_child);
                el.unset_dirty_descendants();
            }
        }
    }

    #[inline]
    fn needs_postorder_traversal() -> bool {
        false
    }

    fn process_postorder(&self, _style_context: &mut StyleContext<E>, _node: E::ConcreteNode) {
        panic!("this should never be called")
    }

    fn text_node_needs_traversal(node: E::ConcreteNode, parent_data: &ElementData) -> bool {
        // for now, traverse text nodes
        true
        // node.get_style_and_layout_data().is_none() || !parent_data.damage.is_empty()
    }

    fn shared_context(&self) -> &SharedStyleContext {
        &self.context
    }
}
