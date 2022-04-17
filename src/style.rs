use cssparser::{Parser, ParserInput};
use dioxus::native_core::node_ref::{AttributeMask, NodeMask, NodeView};
use dioxus::native_core::state::ParentDepState;
use dioxus::native_core_macro::sorted_str_slice;
use parcel_css::properties::border_radius::BorderRadius;
use parcel_css::values::color::CssColor;
use parcel_css::values::rect::Rect;
use parcel_css::{properties::Property, stylesheet::ParserOptions};

#[derive(Clone, PartialEq, Debug)]
pub struct Style {
    pub color: CssColor,
    pub bg_color: CssColor,
    pub border_color: Rect<CssColor>,
    pub border_radius: BorderRadius,
}

impl ParentDepState for Style {
    type Ctx = ();
    type DepState = Self;
    const NODE_MASK: NodeMask =
        NodeMask::new_with_attrs(AttributeMask::Static(SORTED_STYLE_ATTRIBUTES));

    fn reduce(&mut self, node: NodeView<'_>, parent: Option<&Self>, _: &Self::Ctx) -> bool {
        let mut new = Style::default();

        // pass down some attributes from the parent
        if let Some(parent) = parent {
            new.color = parent.color.clone();
        }

        for a in node.attributes() {
            let mut value = ParserInput::new(&a.value);
            let mut parser = Parser::new(&mut value);
            match Property::parse(a.name.into(), &mut parser, &ParserOptions::default()).unwrap() {
                Property::Color(c) => {
                    new.color = c;
                }
                Property::BackgroundColor(c) => {
                    new.bg_color = c;
                }
                Property::BorderColor(c) => {
                    new.border_color = c;
                }
                Property::BorderRadius(r, _) => {
                    new.border_radius = r;
                }
                Property::BorderTopLeftRadius(r, _) => {
                    new.border_radius.top_left = r;
                }
                Property::BorderTopRightRadius(r, _) => {
                    new.border_radius.top_right = r;
                }
                Property::BorderBottomRightRadius(r, _) => {
                    new.border_radius.bottom_right = r;
                }
                Property::BorderBottomLeftRadius(r, _) => {
                    new.border_radius.bottom_left = r;
                }
                _ => {}
            }
        }

        if self != &new {
            *self = new;
            true
        } else {
            false
        }
    }
}

impl Default for Style {
    fn default() -> Self {
        use cssparser::RGBA;
        Style {
            color: CssColor::RGBA(RGBA::new(0, 0, 0, 255)),
            bg_color: CssColor::RGBA(RGBA::new(255, 255, 255, 255)),
            border_color: Rect::default(),
            border_radius: BorderRadius::default(),
        }
    }
}

const SORTED_STYLE_ATTRIBUTES: &[&str] = &sorted_str_slice!([
    "color",
    "background-color",
    "border-color",
    "border-radius",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-bottom-right-radius",
    "border-bottom-left-radius",
]);
