use cssparser::{Parser, ParserInput, RGBA};
use dioxus_native_core::node_ref::{AttributeMask, NodeMask, NodeView};
use dioxus_native_core::state::NodeDepState;
use dioxus_native_core::state::ParentDepState;
use dioxus_native_core_macro::sorted_str_slice;
use lightningcss::properties::border::BorderColor;
use lightningcss::properties::border::BorderSideWidth;
use lightningcss::properties::border::BorderWidth;
use lightningcss::properties::border_radius::BorderRadius;
use lightningcss::traits::Parse;
use lightningcss::values::color::CssColor;
use lightningcss::{properties::Property, stylesheet::ParserOptions};

#[derive(Clone, PartialEq, Debug)]
pub(crate) struct BackgroundColor(pub CssColor);

impl Default for BackgroundColor {
    fn default() -> Self {
        BackgroundColor(CssColor::RGBA(RGBA::new(255, 255, 255, 0)))
    }
}

impl NodeDepState for BackgroundColor {
    type DepState = ();
    type Ctx = ();

    const NODE_MASK: NodeMask =
        NodeMask::new_with_attrs(AttributeMask::Static(&["background-color"]));

    fn reduce(&mut self, node: NodeView<'_>, _sibling: (), _: &Self::Ctx) -> bool {
        if let Some(color_attr) = node.attributes().into_iter().flatten().next() {
            if let Some(as_text) = color_attr.value.as_text() {
                let mut value = ParserInput::new(as_text);
                let mut parser = Parser::new(&mut value);
                if let Ok(new_color) = CssColor::parse(&mut parser) {
                    if self.0 != new_color {
                        *self = Self(new_color);
                        return true;
                    }
                }
            }
        }
        false
    }
}

#[derive(Clone, PartialEq, Debug)]
pub(crate) struct ForgroundColor(pub CssColor);

impl Default for ForgroundColor {
    fn default() -> Self {
        ForgroundColor(CssColor::RGBA(RGBA::new(0, 0, 0, 255)))
    }
}

impl ParentDepState for ForgroundColor {
    type Ctx = ();
    type DepState = (Self,);
    const NODE_MASK: NodeMask = NodeMask::new_with_attrs(AttributeMask::Static(&["color"]));

    fn reduce(&mut self, node: NodeView<'_>, parent: Option<(&Self,)>, _: &Self::Ctx) -> bool {
        let new = if let Some((parent,)) = parent {
            parent.0.clone()
        } else if let Some(color_attr) = node.attributes().into_iter().flatten().next() {
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
}

#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Border {
    pub colors: BorderColor,
    pub width: BorderWidth,
    pub radius: BorderRadius,
}

impl NodeDepState for Border {
    type DepState = ();
    type Ctx = ();

    const NODE_MASK: NodeMask =
        NodeMask::new_with_attrs(AttributeMask::Static(&sorted_str_slice!([
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
            "border-width"
            "border-top-width"
            "border-right-width"
            "border-bottom-width"
            "border-left-width"
        ])));

    fn reduce(&mut self, node: NodeView<'_>, _sibling: (), _: &Self::Ctx) -> bool {
        let mut new = Border::default();
        if let Some(attributes) = node.attributes() {
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
