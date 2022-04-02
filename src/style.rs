use cssparser::{Parser, ParserInput};
use dioxus::native_core::real_dom::PushedDownState;
use dioxus::prelude::*;
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

impl Style {}

impl PushedDownState for Style {
    type Ctx = ();
    fn reduce(
        &mut self,
        parent: Option<&Self>,
        vnode: &dioxus::prelude::VNode<'_>,
        _: &mut Self::Ctx,
    ) {
        *self = Style::default();

        // pass down some attributes from the parent
        if let Some(parent) = parent {
            self.color = parent.color.clone();
        }

        match vnode {
            VNode::Element(el) => {
                for a in el.attributes {
                    let mut value = ParserInput::new(&a.value);
                    let mut parser = Parser::new(&mut value);
                    match Property::parse(a.name.into(), &mut parser, &ParserOptions::default())
                        .unwrap()
                    {
                        Property::Color(c) => {
                            self.color = c;
                        }
                        Property::BackgroundColor(c) => {
                            self.bg_color = c;
                        }
                        Property::BorderColor(c) => {
                            self.border_color = c;
                        }
                        Property::BorderRadius(r, _) => {
                            self.border_radius = r;
                        }
                        Property::BorderTopLeftRadius(r, _) => {
                            self.border_radius.top_left = r;
                        }
                        Property::BorderTopRightRadius(r, _) => {
                            self.border_radius.top_right = r;
                        }
                        Property::BorderBottomRightRadius(r, _) => {
                            self.border_radius.bottom_right = r;
                        }
                        Property::BorderBottomLeftRadius(r, _) => {
                            self.border_radius.bottom_left = r;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
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
