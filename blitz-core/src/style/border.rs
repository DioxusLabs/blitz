use cssparser::{Parser, ParserInput};
use dioxus_native_core::prelude::*;
use dioxus_native_core_macro::partial_derive_state;
use lightningcss::properties::border::BorderColor;
use lightningcss::properties::border::BorderSideWidth;
use lightningcss::properties::border::BorderWidth;
use lightningcss::properties::border_radius::BorderRadius;
use lightningcss::values::color::CssColor;
use lightningcss::{properties::Property, stylesheet::ParserOptions};
use shipyard::Component;

#[derive(Clone, PartialEq, Debug, Component)]
pub(crate) struct Border {
    pub colors: BorderColor,
    pub width: BorderWidth,
    pub radius: BorderRadius,
}

#[partial_derive_state]
impl State for Border {
    type ChildDependencies = ();
    type ParentDependencies = ();
    type NodeDependencies = ();

    const NODE_MASK: NodeMaskBuilder<'static> =
        NodeMaskBuilder::new().with_attrs(AttributeMaskBuilder::Some(&[
            "border-color",
            "border-top-color",
            "border-right-color",
            "border-bottom-color",
            "border-left-color",
            "border-radius",
            "border-top-left-radius",
            "border-top-right-radius",
            "border-bottom-right-radius",
            "border-bottom-left-radius",
            "border-width",
            "border-top-width",
            "border-right-width",
            "border-bottom-width",
            "border-left-width",
        ]));

    fn update<'a>(
        &mut self,
        node_view: NodeView,
        _: <Self::NodeDependencies as Dependancy>::ElementBorrowed<'a>,
        _: Option<<Self::ParentDependencies as Dependancy>::ElementBorrowed<'a>>,
        _: Vec<<Self::ChildDependencies as Dependancy>::ElementBorrowed<'a>>,
        _: &SendAnyMap,
    ) -> bool {
        let mut new = Border::default();
        if let Some(attributes) = node_view.attributes() {
            for a in attributes {
                let mut value = ParserInput::new(a.value.as_text().unwrap());
                let mut parser = Parser::new(&mut value);
                match Property::parse(
                    a.attribute.name.as_str().into(),
                    &mut parser,
                    &ParserOptions::default(),
                )
                .unwrap()
                {
                    Property::BorderColor(c) => {
                        new.colors = c;
                    }
                    Property::BorderTopColor(c) => {
                        new.colors.top = c;
                    }
                    Property::BorderRightColor(c) => {
                        new.colors.right = c;
                    }
                    Property::BorderBottomColor(c) => {
                        new.colors.bottom = c;
                    }
                    Property::BorderLeftColor(c) => {
                        new.colors.left = c;
                    }
                    Property::BorderRadius(r, _) => {
                        new.radius = r;
                    }
                    Property::BorderTopLeftRadius(r, _) => {
                        new.radius.top_left = r;
                    }
                    Property::BorderTopRightRadius(r, _) => {
                        new.radius.top_right = r;
                    }
                    Property::BorderBottomRightRadius(r, _) => {
                        new.radius.bottom_right = r;
                    }
                    Property::BorderBottomLeftRadius(r, _) => {
                        new.radius.bottom_left = r;
                    }
                    Property::BorderWidth(width) => {
                        new.width = width;
                    }
                    Property::BorderTopWidth(width) => {
                        new.width.top = width;
                    }
                    Property::BorderRightWidth(width) => {
                        new.width.right = width;
                    }
                    Property::BorderBottomWidth(width) => {
                        new.width.bottom = width;
                    }
                    Property::BorderLeftWidth(width) => {
                        new.width.left = width;
                    }
                    _ => {}
                }
            }
        }

        if self != &mut new {
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
        children: Vec<<Self::ChildDependencies as Dependancy>::ElementBorrowed<'a>>,
        context: &SendAnyMap,
    ) -> Self {
        let mut myself = Self::default();
        myself.update(node_view, node, parent, children, context);
        myself
    }
}

impl Default for Border {
    fn default() -> Self {
        Border {
            colors: BorderColor {
                top: CssColor::default(),
                right: CssColor::default(),
                bottom: CssColor::default(),
                left: CssColor::default(),
            },
            radius: BorderRadius::default(),
            width: BorderWidth {
                top: BorderSideWidth::default(),
                right: BorderSideWidth::default(),
                bottom: BorderSideWidth::default(),
                left: BorderSideWidth::default(),
            },
        }
    }
}
