#![allow(clippy::module_inception)]

mod attributes;
#[cfg(feature = "custom-widget")]
mod custom_widget;
mod element;
mod node;
pub(crate) mod scrollbar;
mod stylo_data;
#[cfg(feature = "svg")]
mod svg;
mod text;

pub use attributes::{Attribute, Attributes};
#[cfg(feature = "custom-widget")]
pub use custom_widget::{
    ComputedStyles, CustomWidgetData, CustomWidgetStatus, ProxyRenderContext, Widget,
};
pub use element::{
    CanvasData, DocumentData, ElementData, ImageData, ImageResourceData, ListItemLayout,
    ListItemLayoutPosition, Marker, RasterImageData, SpecialElementData, SpecialElementType,
    Status,
};
pub use node::*;
pub use scrollbar::{ScrollbarColor, ScrollbarRef, ScrollbarWidth};
#[cfg(feature = "svg")]
pub use svg::{SvgImageData, SvgIntrinsicDimensions};
pub use text::{GeneratedTextInputEvent, TextBrush, TextInputData, TextLayout};
