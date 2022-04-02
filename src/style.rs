use cssparser::{Parser, ParserInput};
use dioxus::native_core::real_dom::PushedDownState;
use dioxus::prelude::*;
use parcel_css::values::color::CssColor;
use parcel_css::{properties::Property, stylesheet::ParserOptions};

#[derive(Clone, PartialEq, Default, Debug)]
pub struct Style {
    color: CssColor,
    bg_color: CssColor,
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
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}
