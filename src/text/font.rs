use std::sync::{Arc, Mutex};

use dioxus_native_core::{
    node::OwnedAttributeValue,
    node_ref::{AttributeMask, NodeView},
    state::ParentDepState,
    NodeMask,
};
use lightningcss::{
    properties::font::{AbsoluteFontSize, FontSize as FontSizeProperty, RelativeFontSize},
    traits::Parse,
    values::{length::LengthValue, percentage::DimensionPercentage},
};
use taffy::prelude::*;

use super::TextContext;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontSize(pub f32);

pub const DEFAULT_FONT_SIZE: f32 = 16.0;

impl Default for FontSize {
    fn default() -> Self {
        FontSize(DEFAULT_FONT_SIZE)
    }
}

impl ParentDepState for FontSize {
    type Ctx = (Arc<Mutex<Taffy>>, Arc<Mutex<TextContext>>);
    type DepState = (Self,);
    const NODE_MASK: NodeMask = NodeMask::new_with_attrs(AttributeMask::Static(&["font-size"]));

    fn reduce<'a>(
        &mut self,
        node: NodeView<'a, ()>,
        parent: Option<(&'a Self,)>,
        (_, text_context): &Self::Ctx,
    ) -> bool {
        let old = *self;
        let parent = if let Some((parent,)) = parent {
            *self = *parent;
            parent.0
        } else {
            DEFAULT_FONT_SIZE
        };
        let mut has_font_size = false;
        if let Some(attrs) = node.attributes() {
            for attr in attrs {
                match attr.attribute.name.as_str() {
                    "font-size" => {
                        parse_font_size_from_attr(attr.value, parent, DEFAULT_FONT_SIZE).map(|v| {
                            // Currently, only leaf node will have text content, so we don't need to store font size for inner nodes.
                            has_font_size = true;
                            *self = FontSize(v)
                        });
                        break;
                    }
                    _ => {}
                }
            }
        }
        if !has_font_size {
            // use parent font size
            text_context
                .lock()
                .unwrap()
                .set_font_size(node.node_id().0, parent);
        }
        *self != old
    }
}

fn parse_font_size_from_attr(
    css_value: &OwnedAttributeValue,
    parent_font_size: f32,
    root_font_size: f32,
) -> Option<f32> {
    match css_value {
        OwnedAttributeValue::Text(n) => {
            // css font-size parse.
            // not support
            // 1. calc,
            // 3. relative font size. (smaller, larger)
            match FontSizeProperty::parse_string(n) {
                Ok(FontSizeProperty::Length(length)) => match length {
                    DimensionPercentage::Dimension(l) => match l {
                        LengthValue::Rem(v) => Some(v * root_font_size),
                        LengthValue::Em(v) => Some(v * parent_font_size),
                        _ => l.to_px().map(|v| v as f32),
                    },
                    // same with em.
                    DimensionPercentage::Percentage(p) => Some(p.0 * parent_font_size),
                    DimensionPercentage::Calc(_c) => None,
                },
                Ok(FontSizeProperty::Absolute(abs_val)) => {
                    let factor = match abs_val {
                        AbsoluteFontSize::XXSmall => 0.6,
                        AbsoluteFontSize::XSmall => 0.75,
                        AbsoluteFontSize::Small => 0.89, // 8/9
                        AbsoluteFontSize::Medium => 1.0,
                        AbsoluteFontSize::Large => 1.25,
                        AbsoluteFontSize::XLarge => 1.5,
                        AbsoluteFontSize::XXLarge => 2.0,
                    };
                    Some(factor * root_font_size)
                }
                Ok(FontSizeProperty::Relative(rel_val)) => {
                    let factor = match rel_val {
                        RelativeFontSize::Smaller => 0.8,
                        RelativeFontSize::Larger => 1.25,
                    };
                    Some(factor * parent_font_size)
                }
                _ => None,
            }
        }
        OwnedAttributeValue::Float(n) => Some(n.to_owned() as f32),
        OwnedAttributeValue::Int(n) => Some(n.to_owned() as f32),
        _ => None,
    }
}
