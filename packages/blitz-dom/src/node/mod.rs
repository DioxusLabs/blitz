#![allow(clippy::module_inception)]

mod attributes;
#[cfg(feature = "shadow-dom")]
mod custom_element;
#[cfg(feature = "custom-widget")]
mod custom_widget;
mod element;
mod node;
mod stylo_data;

pub use attributes::{Attribute, Attributes};
#[cfg(feature = "shadow-dom")]
pub use custom_element::{
    CustomElement, CustomElementCtx, CustomElementData, CustomElementDefinition,
    CustomElementFactory, CustomElementRegistry,
};
#[cfg(feature = "custom-widget")]
pub use custom_widget::{
    ComputedStyles, CustomWidgetData, CustomWidgetStatus, ProxyRenderContext, Widget,
};
pub use element::{
    CanvasData, ElementData, ImageData, ImageResourceData, ListItemLayout, ListItemLayoutPosition,
    Marker, RasterImageData, SpecialElementData, SpecialElementType, Status, TextBrush,
    TextInputData, TextLayout,
};
pub use node::*;
