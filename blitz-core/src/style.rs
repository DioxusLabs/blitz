use cssparser::{Parser, ParserInput, RGBA};
use dioxus_native_core::node::OwnedAttributeValue;
use dioxus_native_core::prelude::*;
use dioxus_native_core_macro::partial_derive_state;
use lightningcss::properties::border::BorderColor;
use lightningcss::properties::border::BorderSideWidth;
use lightningcss::properties::border::BorderWidth;
use lightningcss::properties::border_radius::BorderRadius;
use lightningcss::properties::font::AbsoluteFontSize;
use lightningcss::properties::font::RelativeFontSize;
use lightningcss::traits::Parse;
use lightningcss::values::color::CssColor;
use lightningcss::values::length::LengthValue;
use lightningcss::values::percentage::DimensionPercentage;
use lightningcss::{
    properties::font::FontSize as FontSizeProperty, properties::Property, stylesheet::ParserOptions,
};
use shipyard::Component;

#[derive(Clone, PartialEq, Debug, Component)]
pub(crate) struct BackgroundColor(pub CssColor);

impl Default for BackgroundColor {
    fn default() -> Self {
        BackgroundColor(CssColor::RGBA(RGBA::new(255, 255, 255, 0)))
    }
}

#[partial_derive_state]
impl State for BackgroundColor {
    type ChildDependencies = ();
    type ParentDependencies = ();
    type NodeDependencies = ();

    const NODE_MASK: NodeMaskBuilder<'static> =
        NodeMaskBuilder::new().with_attrs(AttributeMaskBuilder::Some(&["background-color"]));

    fn update<'a>(
        &mut self,
        node_view: NodeView,
        _: <Self::NodeDependencies as Dependancy>::ElementBorrowed<'a>,
        _: Option<<Self::ParentDependencies as Dependancy>::ElementBorrowed<'a>>,
        _: Vec<<Self::ChildDependencies as Dependancy>::ElementBorrowed<'a>>,
        _: &SendAnyMap,
    ) -> bool {
        if let Some(color_attr) = node_view.attributes().into_iter().flatten().next() {
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

#[derive(Clone, PartialEq, Debug, Component)]
pub(crate) struct FontSize(pub f32);
pub const DEFAULT_FONT_SIZE: f32 = 16.0;

impl Default for FontSize {
    fn default() -> Self {
        FontSize(DEFAULT_FONT_SIZE)
    }
}

#[partial_derive_state]
impl State for FontSize {
    type ChildDependencies = ();
    type ParentDependencies = (Self,);
    type NodeDependencies = ();

    const NODE_MASK: NodeMaskBuilder<'static> =
        NodeMaskBuilder::new().with_attrs(AttributeMaskBuilder::Some(&["font-size"]));

    fn update<'a>(
        &mut self,
        node_view: NodeView,
        _: <Self::NodeDependencies as Dependancy>::ElementBorrowed<'a>,
        parent: Option<<Self::ParentDependencies as Dependancy>::ElementBorrowed<'a>>,
        _: Vec<<Self::ChildDependencies as Dependancy>::ElementBorrowed<'a>>,
        _: &SendAnyMap,
    ) -> bool {
        let new = if let Some(size_attr) = node_view.attributes().into_iter().flatten().next() {
            let parent_size = if let Some((parent_size,)) = parent {
                parent_size.0
            } else {
                DEFAULT_FONT_SIZE
            };
            if let Some(font_size) =
                parse_font_size_from_attr(size_attr.value, parent_size, DEFAULT_FONT_SIZE)
            {
                font_size
            } else {
                DEFAULT_FONT_SIZE
            }
        } else if let Some((parent_size,)) = parent {
            parent_size.0
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
                        _ => l.to_px(),
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
