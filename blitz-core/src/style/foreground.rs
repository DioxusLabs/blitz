use cssparser::{Parser, ParserInput, RGBA};
use dioxus_native_core::prelude::*;
use dioxus_native_core_macro::partial_derive_state;
use lightningcss::traits::Parse;
use lightningcss::values::color::CssColor;
use shipyard::Component;

#[derive(Clone, PartialEq, Debug, Component)]
pub(crate) struct ForgroundColor(pub CssColor);

impl Default for ForgroundColor {
    fn default() -> Self {
        ForgroundColor(CssColor::RGBA(RGBA::new(0, 0, 0, 255)))
    }
}

#[partial_derive_state]
impl State for ForgroundColor {
    type ChildDependencies = ();
    type ParentDependencies = (Self,);
    type NodeDependencies = ();
    const NODE_MASK: NodeMaskBuilder<'static> =
        NodeMaskBuilder::new().with_attrs(AttributeMaskBuilder::Some(&["color"]));

    fn update<'a>(
        &mut self,
        node_view: NodeView,
        _: <Self::NodeDependencies as Dependancy>::ElementBorrowed<'a>,
        parent: Option<<Self::ParentDependencies as Dependancy>::ElementBorrowed<'a>>,
        _: Vec<<Self::ChildDependencies as Dependancy>::ElementBorrowed<'a>>,
        _: &SendAnyMap,
    ) -> bool {
        let new = if let Some(color_attr) = node_view.attributes().into_iter().flatten().next() {
            if let Some(as_text) = color_attr.value.as_text() {
                let mut value = ParserInput::new(as_text);
                let mut parser = Parser::new(&mut value);
                if let Ok(new_color) = CssColor::parse(&mut parser) {
                    new_color
                } else {
                    return false;
                }
            } else {
                return false;
            }
        } else if let Some((parent,)) = parent {
            parent.0.clone()
        } else {
            return false;
        };

        if self.0 != new {
            *self = Self(new);
            true
        } else {
            false
        }
    }

    fn create<'a>(
        node_view: NodeView<()>,
        node: <Self::NodeDependencies as Dependancy>::ElementBorrowed<'a>,
        parent: Option<<Self::ParentDependencies as Dependancy>::ElementBorrowed<'a>>,
        children: Vec<<Self::ChildDependencies as Dependancy>::ElementBorrowed<'a>>,
        context: &SendAnyMap,
    ) -> Self {
        let mut myself = Self::default();
        myself.update(node_view, node, parent, children, context);
        myself
    }
}
