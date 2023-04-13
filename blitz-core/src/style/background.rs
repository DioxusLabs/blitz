use cssparser::{Parser, ParserInput, RGBA};
use dioxus_native_core::prelude::*;
use dioxus_native_core_macro::partial_derive_state;
use lightningcss::properties::background;
use lightningcss::properties::border::BorderColor;
use lightningcss::properties::border::BorderSideWidth;
use lightningcss::properties::border::BorderWidth;
use lightningcss::properties::border_radius::BorderRadius;
use lightningcss::traits::Parse;
use lightningcss::values::color::CssColor;
use lightningcss::values::gradient;
use lightningcss::values::gradient::GradientItem;
use lightningcss::values::length::LengthPercentage;
use lightningcss::values::length::LengthValue;
use lightningcss::{properties::Property, stylesheet::ParserOptions};
use shipyard::Component;
use taffy::prelude::Size;
use vello::peniko;
use vello::peniko::Color;

use crate::util::angle_to_turn_percentage;
use crate::util::map_dimension_percentage;
use crate::util::translate_color;
use crate::util::Resolve;

#[derive(Clone, PartialEq, Debug)]
enum GradientType {
    Linear,
    Radial,
    Conic,
}

#[derive(Clone, PartialEq, Debug)]
struct ColorStop {
    color: Color,
    position: Option<LengthPercentage>,
}

impl TryFrom<&GradientItem<LengthPercentage>> for ColorStop {
    type Error = ();

    fn try_from(item: &GradientItem<LengthPercentage>) -> Result<Self, Self::Error> {
        match item {
            GradientItem::ColorStop(color_stop) => Ok(ColorStop {
                color: translate_color(&color_stop.color),
                position: color_stop.position.clone(),
            }),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Gradient {
    gradient_type: GradientType,
    stops: Vec<ColorStop>,
    // The last size and resolved positions and stops. This is used cache the last resolved position and stops so that we don't have to resolve them again if they are the same.
    last_resolved: Option<GradientCache>,
    repeating: bool,
}

impl TryFrom<gradient::Gradient> for Gradient {
    type Error = ();

    fn try_from(gradient: gradient::Gradient) -> Result<Self, Self::Error> {
        use gradient::Gradient::*;
        let (gradient_type, repeating) = match gradient {
            Linear(_) => (GradientType::Linear, false),
            RepeatingLinear(_) => (GradientType::Linear, true),
            Radial(_) => (GradientType::Radial, false),
            RepeatingRadial(_) => (GradientType::Radial, true),
            Conic(_) => (GradientType::Conic, false),
            RepeatingConic(_) => (GradientType::Conic, true),
            _ => return Err(()),
        };
        let stops = match gradient {
            Linear(linear) | RepeatingLinear(linear) => linear.items,
            Radial(radial) | RepeatingRadial(radial) => radial.items,
            Conic(conic) | RepeatingConic(conic) => conic
                .items
                .into_iter()
                .map(|item| match item {
                    GradientItem::ColorStop(stop) => GradientItem::ColorStop(gradient::ColorStop {
                        color: stop.color,
                        position: stop.position.map(|percentage| {
                            map_dimension_percentage(percentage, |angle| {
                                LengthValue::Px(angle_to_turn_percentage(angle))
                            })
                        }),
                    }),
                    GradientItem::Hint(pos) => {
                        GradientItem::Hint(map_dimension_percentage(pos, |angle| {
                            LengthValue::Px(angle_to_turn_percentage(angle))
                        }))
                    }
                })
                .collect(),
            _ => return Err(()),
        };

        let stops = stops
            .into_iter()
            .map(|stop| ColorStop::try_from(&stop))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Gradient {
            gradient_type,
            stops,
            last_resolved: None,
            repeating,
        })
    }
}

#[derive(Clone, Debug)]
struct GradientCache {
    rect: Size<f32>,
    viewport_size: Size<u32>,
    result: peniko::ColorStops,
}

impl PartialEq for Gradient {
    fn eq(&self, other: &Self) -> bool {
        self.gradient_type == other.gradient_type && self.stops == other.stops
    }
}

impl Gradient {
    /// Resolve all missing positions. If a position is missing, it is in between the last resolved position and the next resolved position.
    pub(crate) fn resolve_stops(
        &mut self,
        rect: &Size<f32>,
        viewport_size: &Size<u32>,
    ) -> &peniko::ColorStops {
        let resolved = self
            .last_resolved
            .as_ref()
            .filter(|last_resolved| {
                last_resolved.rect == *rect && last_resolved.viewport_size == *viewport_size
            })
            .is_some();
        if resolved {
            &self.last_resolved.as_ref().unwrap().result
        } else {
            let mut resolved = smallvec::SmallVec::default();

            let mut last_resolved_position = 0.0f32;
            let mut unresolved_positions: Vec<Color> = Vec::new();
            for stop in &mut self.stops {
                match &stop.position {
                    Some(position) => {
                        let position =
                            position.resolve(crate::util::Axis::X, rect, viewport_size) as f32;

                        let total_unresolved = unresolved_positions.len();
                        for (i, unresolved_stop) in unresolved_positions.iter_mut().enumerate() {
                            let offset = last_resolved_position
                                + ((i + 1) / (total_unresolved + 1)) as f32
                                    * (position - last_resolved_position);

                            resolved.push(peniko::ColorStop {
                                color: *unresolved_stop,
                                offset,
                            });
                        }
                        unresolved_positions.clear();
                        last_resolved_position = position;
                        resolved.push(peniko::ColorStop {
                            color: stop.color,
                            offset: position,
                        });
                    }
                    None => {
                        unresolved_positions.push(stop.color);
                    }
                }
            }

            self.last_resolved = Some(GradientCache {
                rect: *rect,
                viewport_size: *viewport_size,
                result: resolved,
            });

            &self.last_resolved.as_ref().unwrap().result
        }
    }
}

#[derive(Clone, PartialEq, Debug, Default)]
pub(crate) enum Image {
    #[default]
    None,
    Url(String),
    Gradient(Gradient),
}

impl<'a> TryFrom<lightningcss::values::image::Image<'a>> for Image {
    type Error = ();

    fn try_from(value: lightningcss::values::image::Image<'a>) -> Result<Self, Self::Error> {
        use lightningcss::values::image::Image::*;
        match value {
            None => Ok(Image::None),
            Url(url) => Ok(Image::Url(url.url.to_string())),
            Gradient(gradient) => Ok(Image::Gradient((*gradient).try_into()?)),
            _ => Err(()),
        }
    }
}

#[derive(Clone, PartialEq, Debug, Default)]
pub(crate) struct Repeat {
    x: bool,
    y: bool,
}

impl From<background::BackgroundRepeat> for Repeat {
    fn from(repeat: background::BackgroundRepeat) -> Self {
        fn is_repeat(repeat: &background::BackgroundRepeatKeyword) -> bool {
            use background::BackgroundRepeatKeyword::*;
            !matches!(repeat, NoRepeat)
        }

        Repeat {
            x: is_repeat(&repeat.x),
            y: is_repeat(&repeat.y),
        }
    }
}

#[derive(Clone, PartialEq, Debug, Component)]
pub(crate) struct Background {
    pub color: Color,
    pub image: Image,
    pub repeat: Repeat,
}

impl Default for Background {
    fn default() -> Self {
        Background {
            color: Color::rgba8(255, 255, 255, 0),
            image: Image::default(),
            repeat: Repeat::default(),
        }
    }
}

#[partial_derive_state]
impl State for Background {
    type ChildDependencies = ();
    type ParentDependencies = ();
    type NodeDependencies = ();

    const NODE_MASK: NodeMaskBuilder<'static> =
        NodeMaskBuilder::new().with_attrs(AttributeMaskBuilder::Some(&[
            "background",
            "background-color",
            "background-image",
            "background-repeat",
        ]));

    fn update<'a>(
        &mut self,
        node_view: NodeView,
        _: <Self::NodeDependencies as Dependancy>::ElementBorrowed<'a>,
        _: Option<<Self::ParentDependencies as Dependancy>::ElementBorrowed<'a>>,
        _: Vec<<Self::ChildDependencies as Dependancy>::ElementBorrowed<'a>>,
        _: &SendAnyMap,
    ) -> bool {
        let mut new = Background::default();
        for attr in node_view.attributes().into_iter().flatten() {
            if let Some(attr_value) = attr.value.as_text() {
                match attr.attribute.name.as_str() {
                    "background" => {
                        if let Ok(background) = background::Background::parse_string(attr_value) {
                            new.color = translate_color(&background.color);
                            new.repeat = background.repeat.into();
                            new.image = background.image.try_into().expect(
                                "attempted to convert a background Blitz does not support yet",
                            );
                        }
                    }
                    "background-color" => {
                        if let Ok(new_color) = CssColor::parse_string(attr_value) {
                            new.color = translate_color(&new_color);
                        }
                    }
                    "background-image" => {}
                    "background-repeat" => {
                        if let Ok(repeat) = background::BackgroundRepeat::parse_string(attr_value) {
                            new.repeat = repeat.into();
                        }
                    }
                    _ => {}
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
