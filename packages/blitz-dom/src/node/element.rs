use cssparser::ParserInput;
use linebender_resource_handle::Blob;
use markup5ever::{LocalName, QualName, local_name};
use selectors::matching::QuirksMode;
use std::str::FromStr;
use std::sync::Arc;
use style::Atom;
use style::parser::ParserContext;
use style::properties::{Importance, PropertyDeclaration, PropertyId, SourcePropertyDeclaration};
use style::stylesheets::{DocumentStyleSheet, Origin, UrlExtraData};
use style::{
    properties::{PropertyDeclarationBlock, parse_style_attribute},
    servo_arc::Arc as ServoArc,
    shared_lock::{Locked, SharedRwLock},
    stylesheets::CssRuleType,
};
use style_traits::ParsingMode;
use url::Url;

use super::{Attribute, Attributes};
use crate::Document;
use crate::layout::table::TableContext;
use crate::node::{TextBrush, TextInputData, TextLayout};

#[cfg(feature = "custom-widget")]
use super::custom_widget::CustomWidgetData;

macro_rules! local_names {
    ($($name:tt),+) => {
        [$(local_name!($name),)+]
    };
}

#[derive(Debug, Clone)]
pub struct ElementData {
    /// The elements tag name, namespace and prefix
    pub name: QualName,

    /// The elements id attribute parsed as an atom (if it has one)
    pub id: Option<Atom>,

    /// The element's attributes
    pub attrs: Attributes,

    /// Whether the element is focussable
    pub is_focussable: bool,

    /// The element's parsed style attribute (used by stylo)
    pub style_attribute: Option<ServoArc<Locked<PropertyDeclarationBlock>>>,

    /// Heterogeneous data that depends on the element's type.
    /// For example:
    ///   - The image data for \<img\> elements.
    ///   - The parley Layout for inline roots.
    ///   - The text editor for input/textarea elements
    pub special_data: SpecialElementData,

    pub background_images: Vec<Option<ImageResourceData>>,

    pub mask_images: Vec<Option<ImageResourceData>>,

    /// Parley text layout (elements with inline inner display mode only)
    pub inline_layout_data: Option<Box<TextLayout>>,

    /// Data associated with display: list-item. Note that this display mode
    /// does not exclude inline_layout_data
    pub list_item_data: Option<Box<ListItemLayout>>,

    /// The element's template contents (\<template\> elements only)
    pub template_contents: Option<usize>,
    // /// Whether the node is a [HTML integration point] (https://html.spec.whatwg.org/multipage/#html-integration-point)
    // pub mathml_annotation_xml_integration_point: bool,
}

#[derive(Copy, Clone, Default)]
#[non_exhaustive]
pub enum SpecialElementType {
    Stylesheet,
    Image,
    Canvas,
    TableRoot,
    TextInput,
    CheckboxInput,
    #[cfg(feature = "file-input")]
    FileInput,
    #[default]
    None,
}

/// Heterogeneous data that depends on the element's type.
#[derive(Default)]
pub enum SpecialElementData {
    /// A sub-document such an \<iframe\> or \<web-view\> element
    SubDocument(Box<dyn Document>),
    /// A custom widget
    #[cfg(feature = "custom-widget")]
    CustomWidget(CustomWidgetData),
    /// A stylesheet
    Stylesheet(DocumentStyleSheet),
    /// An \<img\> element's image data
    Image(Box<ImageData>),
    /// A \<canvas\> element's custom paint source
    Canvas(CanvasData),
    /// Pre-computed table layout data
    TableRoot(Arc<TableContext>),
    /// Parley text editor (text inputs)
    TextInput(TextInputData),
    /// Checkbox checked state
    CheckboxInput(bool),
    /// Selected files
    #[cfg(feature = "file-input")]
    FileInput(FileData),
    /// No data (for nodes that don't need any node-specific data)
    #[default]
    None,
}

impl Clone for SpecialElementData {
    fn clone(&self) -> Self {
        match self {
            Self::SubDocument(_) => Self::None, // TODO
            #[cfg(feature = "custom-widget")]
            Self::CustomWidget(_) => Self::None, // TODO
            Self::Stylesheet(data) => Self::Stylesheet(data.clone()),
            Self::Image(data) => Self::Image(data.clone()),
            Self::Canvas(data) => Self::Canvas(data.clone()),
            Self::TableRoot(data) => Self::TableRoot(data.clone()),
            Self::TextInput(data) => Self::TextInput(data.clone()),
            Self::CheckboxInput(data) => Self::CheckboxInput(*data),
            #[cfg(feature = "file-input")]
            Self::FileInput(data) => Self::FileInput(data.clone()),
            Self::None => Self::None,
        }
    }
}

impl SpecialElementData {
    pub fn take(&mut self) -> Self {
        std::mem::take(self)
    }
}

impl ElementData {
    pub fn new(name: QualName, attrs: Vec<Attribute>) -> Self {
        let id_attr_atom = attrs
            .iter()
            .find(|attr| &attr.name.local == "id")
            .map(|attr| attr.value.as_ref())
            .map(|value: &str| Atom::from(value));

        let mut data = ElementData {
            name,
            id: id_attr_atom,
            attrs: Attributes::new(attrs),
            is_focussable: false,
            style_attribute: Default::default(),
            inline_layout_data: None,
            list_item_data: None,
            special_data: SpecialElementData::None,
            template_contents: None,
            background_images: Vec::new(),
            mask_images: Vec::new(),
        };
        data.flush_is_focussable();
        data
    }

    pub fn attrs(&self) -> &[Attribute] {
        &self.attrs
    }

    pub fn attr(&self, name: impl PartialEq<LocalName>) -> Option<&str> {
        let attr = self.attrs.iter().find(|attr| name == attr.name.local)?;
        Some(&attr.value)
    }

    pub fn attr_parsed<T: FromStr>(&self, name: impl PartialEq<LocalName>) -> Option<T> {
        let attr = self.attrs.iter().find(|attr| name == attr.name.local)?;
        attr.value.parse::<T>().ok()
    }

    /// Detects the presence of the attribute, treating *any* value as truthy.
    pub fn has_attr(&self, name: impl PartialEq<LocalName>) -> bool {
        self.attrs.iter().any(|attr| name == attr.name.local)
    }

    pub fn can_be_disabled(&self) -> bool {
        local_names!("button", "input", "select", "textarea").contains(&self.name.local)
    }

    pub fn image_data(&self) -> Option<&ImageData> {
        match &self.special_data {
            SpecialElementData::Image(data) => Some(&**data),
            _ => None,
        }
    }

    pub fn image_data_mut(&mut self) -> Option<&mut ImageData> {
        match self.special_data {
            SpecialElementData::Image(ref mut data) => Some(&mut **data),
            _ => None,
        }
    }

    pub fn raster_image_data(&self) -> Option<&RasterImageData> {
        match self.image_data()? {
            ImageData::Raster(data) => Some(data),
            _ => None,
        }
    }

    pub fn raster_image_data_mut(&mut self) -> Option<&mut RasterImageData> {
        match self.image_data_mut()? {
            ImageData::Raster(data) => Some(data),
            _ => None,
        }
    }

    pub fn canvas_data(&self) -> Option<&CanvasData> {
        match &self.special_data {
            SpecialElementData::Canvas(data) => Some(data),
            _ => None,
        }
    }

    pub fn sub_doc_data(&self) -> Option<&dyn Document> {
        match &self.special_data {
            SpecialElementData::SubDocument(data) => Some(data.as_ref()),
            _ => None,
        }
    }

    pub fn sub_doc_data_mut(&mut self) -> Option<&mut dyn Document> {
        match &mut self.special_data {
            SpecialElementData::SubDocument(data) => Some(data.as_mut()),
            _ => None,
        }
    }

    #[cfg(feature = "svg")]
    pub fn svg_data(&self) -> Option<&usvg::Tree> {
        match self.image_data()? {
            ImageData::Svg(data) => Some(data),
            _ => None,
        }
    }

    pub fn text_input_data(&self) -> Option<&TextInputData> {
        match &self.special_data {
            SpecialElementData::TextInput(data) => Some(data),
            _ => None,
        }
    }

    pub fn text_input_data_mut(&mut self) -> Option<&mut TextInputData> {
        match &mut self.special_data {
            SpecialElementData::TextInput(data) => Some(data),
            _ => None,
        }
    }

    #[cfg(feature = "custom-widget")]
    pub fn custom_widget_data(&self) -> Option<&CustomWidgetData> {
        match &self.special_data {
            SpecialElementData::CustomWidget(data) => Some(data),
            _ => None,
        }
    }

    #[cfg(feature = "custom-widget")]
    pub fn custom_widget_data_mut(&mut self) -> Option<&mut CustomWidgetData> {
        match &mut self.special_data {
            SpecialElementData::CustomWidget(data) => Some(data),
            _ => None,
        }
    }

    pub fn checkbox_input_checked(&self) -> Option<bool> {
        match self.special_data {
            SpecialElementData::CheckboxInput(checked) => Some(checked),
            _ => None,
        }
    }

    pub fn checkbox_input_checked_mut(&mut self) -> Option<&mut bool> {
        match self.special_data {
            SpecialElementData::CheckboxInput(ref mut checked) => Some(checked),
            _ => None,
        }
    }

    #[cfg(feature = "file-input")]
    pub fn file_data(&self) -> Option<&FileData> {
        match &self.special_data {
            SpecialElementData::FileInput(data) => Some(data),
            _ => None,
        }
    }

    #[cfg(feature = "file-input")]
    pub fn file_data_mut(&mut self) -> Option<&mut FileData> {
        match &mut self.special_data {
            SpecialElementData::FileInput(data) => Some(data),
            _ => None,
        }
    }

    pub fn flush_is_focussable(&mut self) {
        let disabled: bool = self.attr_parsed(local_name!("disabled")).unwrap_or(false);
        let tabindex: Option<i32> = self.attr_parsed(local_name!("tabindex"));
        let contains_sub_document: bool = self.sub_doc_data().is_some();

        self.is_focussable = contains_sub_document
            || (!disabled
                && match tabindex {
                    Some(index) => index >= 0,
                    None => {
                        // Some focusable HTML elements have a default tabindex value of 0 set under the hood by the user agent.
                        // These elements are:
                        //   - <a> or <area> with href attribute
                        //   - <button>, <frame>, <iframe>, <input>, <object>, <select>, <textarea>, and SVG <a> element
                        //   - <summary> element that provides summary for a <details> element.

                        if [local_name!("a"), local_name!("area")].contains(&self.name.local) {
                            self.attr(local_name!("href")).is_some()
                        } else {
                            const DEFAULT_FOCUSSABLE_ELEMENTS: [LocalName; 6] = [
                                local_name!("button"),
                                local_name!("input"),
                                local_name!("select"),
                                local_name!("textarea"),
                                local_name!("frame"),
                                local_name!("iframe"),
                            ];
                            DEFAULT_FOCUSSABLE_ELEMENTS.contains(&self.name.local)
                        }
                    }
                })
    }

    pub fn flush_style_attribute(&mut self, guard: &SharedRwLock, url_extra_data: &UrlExtraData) {
        self.style_attribute = self.attr(local_name!("style")).map(|style_str| {
            ServoArc::new(guard.wrap(parse_style_attribute(
                style_str,
                url_extra_data,
                None,
                QuirksMode::NoQuirks,
                CssRuleType::Style,
            )))
        });
    }

    pub fn set_style_property(
        &mut self,
        name: &str,
        value: &str,
        guard: &SharedRwLock,
        url_extra_data: UrlExtraData,
    ) -> bool {
        let context = ParserContext::new(
            Origin::Author,
            &url_extra_data,
            Some(CssRuleType::Style),
            ParsingMode::DEFAULT,
            QuirksMode::NoQuirks,
            /* namespaces = */ Default::default(),
            None,
            None,
            /* attr_taint = */ Default::default(),
        );

        let Ok(property_id) = PropertyId::parse(name, &context) else {
            #[cfg(feature = "tracing")]
            tracing::warn!(property = name, "Unsupported property");
            return false;
        };
        let mut source_property_declaration = SourcePropertyDeclaration::default();
        let mut input = ParserInput::new(value);
        let mut parser = style::values::Parser::new(&mut input);
        let Ok(_) = PropertyDeclaration::parse_into(
            &mut source_property_declaration,
            property_id,
            &context,
            &mut parser,
        ) else {
            #[cfg(feature = "tracing")]
            tracing::warn!(property = name, value, "Invalid property value");
            return false;
        };

        if self.style_attribute.is_none() {
            self.style_attribute = Some(ServoArc::new(guard.wrap(PropertyDeclarationBlock::new())));
        }
        self.style_attribute
            .as_mut()
            .unwrap()
            .write_with(&mut guard.write())
            .extend(source_property_declaration.drain(), Importance::Normal);

        true
    }

    pub fn remove_style_property(
        &mut self,
        name: &str,
        guard: &SharedRwLock,
        url_extra_data: UrlExtraData,
    ) -> bool {
        let context = ParserContext::new(
            Origin::Author,
            &url_extra_data,
            Some(CssRuleType::Style),
            ParsingMode::DEFAULT,
            QuirksMode::NoQuirks,
            /* namespaces = */ Default::default(),
            None,
            None,
            /* attr_taint = */ Default::default(),
        );
        let Ok(property_id) = PropertyId::parse(name, &context) else {
            #[cfg(feature = "tracing")]
            tracing::warn!(property = name, "Unsupported property");
            return false;
        };

        if let Some(style) = &mut self.style_attribute {
            let mut guard = guard.write();
            let style = style.write_with(&mut guard);
            if let Some(index) = style.first_declaration_to_remove(&property_id) {
                style.remove_property(&property_id, index);
                return true;
            }
        }

        false
    }

    pub fn set_sub_document(&mut self, sub_document: Box<dyn Document>) {
        self.special_data = SpecialElementData::SubDocument(sub_document);
    }

    pub fn remove_sub_document(&mut self) {
        self.special_data = SpecialElementData::None;
    }

    #[cfg(feature = "custom-widget")]
    pub fn set_custom_widget(&mut self, widget: Box<dyn crate::Widget>) {
        use crate::node::custom_widget::CustomWidgetData;
        self.special_data = SpecialElementData::CustomWidget(CustomWidgetData::new(widget));
    }

    #[cfg(feature = "custom-widget")]
    pub fn remove_custom_widget(&mut self) -> Vec<anyrender::ResourceId> {
        let resource_ids = self
            .custom_widget_data_mut()
            .map(|widget_data| widget_data.take_resource_ids())
            .unwrap_or_default();
        self.special_data = SpecialElementData::None;
        resource_ids
    }

    pub fn take_inline_layout(&mut self) -> Option<Box<TextLayout>> {
        std::mem::take(&mut self.inline_layout_data)
    }

    pub fn is_submit_button(&self) -> bool {
        if self.name.local != local_name!("button") {
            return false;
        }
        let type_attr = self.attr(local_name!("type"));
        let is_submit = type_attr == Some("submit");
        let is_auto_submit = type_attr.is_none()
            && self.attr(LocalName::from("command")).is_none()
            && self.attr(LocalName::from("commandfor")).is_none();
        is_submit || is_auto_submit
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RasterImageData {
    /// The width of the image
    pub width: u32,
    /// The height of the image
    pub height: u32,
    /// The raw image data in RGBA8 format
    pub data: Blob<u8>,
}
impl RasterImageData {
    pub fn new(width: u32, height: u32, data: Arc<Vec<u8>>) -> Self {
        Self {
            width,
            height,
            data: Blob::new(data),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ImageData {
    Raster(RasterImageData),
    #[cfg(feature = "svg")]
    Svg(Arc<usvg::Tree>),
    None,
}
#[cfg(feature = "svg")]
impl From<usvg::Tree> for ImageData {
    fn from(value: usvg::Tree) -> Self {
        Self::Svg(Arc::new(value))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Ok,
    Error,
    Loading,
}

#[derive(Debug, Clone)]
pub struct ImageResourceData {
    /// The url of the background image
    pub url: ServoArc<Url>,
    /// The loading status of the background image
    pub status: Status,
    /// The image data
    pub image: ImageData,
}

impl ImageResourceData {
    pub fn new(url: ServoArc<Url>) -> Self {
        Self {
            url,
            status: Status::Loading,
            image: ImageData::None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CanvasData {
    pub custom_paint_source_id: u64,
}

impl std::fmt::Debug for SpecialElementData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpecialElementData::SubDocument(_) => f.write_str("NodeSpecificData::SubDocument"),
            #[cfg(feature = "custom-widget")]
            SpecialElementData::CustomWidget(_) => f.write_str("NodeSpecificData::CustomWidget"),
            SpecialElementData::Stylesheet(_) => f.write_str("NodeSpecificData::Stylesheet"),
            SpecialElementData::Image(data) => match **data {
                ImageData::Raster(_) => f.write_str("NodeSpecificData::Image(Raster)"),
                #[cfg(feature = "svg")]
                ImageData::Svg(_) => f.write_str("NodeSpecificData::Image(Svg)"),
                ImageData::None => f.write_str("NodeSpecificData::Image(None)"),
            },
            SpecialElementData::Canvas(_) => f.write_str("NodeSpecificData::Canvas"),
            SpecialElementData::TableRoot(_) => f.write_str("NodeSpecificData::TableRoot"),
            SpecialElementData::TextInput(_) => f.write_str("NodeSpecificData::TextInput"),
            SpecialElementData::CheckboxInput(_) => f.write_str("NodeSpecificData::CheckboxInput"),
            #[cfg(feature = "file-input")]
            SpecialElementData::FileInput(_) => f.write_str("NodeSpecificData::FileInput"),
            SpecialElementData::None => f.write_str("NodeSpecificData::None"),
        }
    }
}

#[derive(Clone)]
pub struct ListItemLayout {
    pub marker: Marker,
    pub position: ListItemLayoutPosition,
}

//We seperate chars from strings in order to optimise rendering - ie not needing to
//construct a whole parley layout for simple char markers
#[derive(Debug, PartialEq, Clone)]
pub enum Marker {
    Char(char),
    String(String),
}

//Value depends on list-style-position, determining whether a seperate layout is created for it
#[derive(Clone)]
pub enum ListItemLayoutPosition {
    Inside,
    Outside(Box<parley::Layout<TextBrush>>),
}

impl std::fmt::Debug for ListItemLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ListItemLayout - marker {:?}", self.marker)
    }
}

#[cfg(feature = "file-input")]
mod file_data {
    use std::ops::{Deref, DerefMut};
    use std::path::PathBuf;

    #[derive(Clone, Debug)]
    pub struct FileData(pub Vec<PathBuf>);
    impl Deref for FileData {
        type Target = Vec<PathBuf>;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }
    impl DerefMut for FileData {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.0
        }
    }
    impl From<Vec<PathBuf>> for FileData {
        fn from(files: Vec<PathBuf>) -> Self {
            Self(files)
        }
    }
}
#[cfg(feature = "file-input")]
pub use file_data::FileData;

#[cfg(test)]
mod tests {
    use super::TextInputData;
    use parley::{FontContext, LayoutContext};

    /// Build a [`TextInputData`] with the given text laid out at scale 1.0.
    fn make_input(is_multiline: bool, text: &str) -> TextInputData {
        let mut font_ctx = FontContext::new();
        let mut layout_ctx = LayoutContext::new();
        let mut data = TextInputData::new(is_multiline);
        data.editor.set_scale(1.0);
        data.editor.set_text(text);
        data.editor
            .driver(&mut font_ctx, &mut layout_ctx)
            .refresh_layout();
        data
    }

    #[test]
    fn short_text_does_not_scroll() {
        let mut data = make_input(false, "hi");
        // A wide content box that comfortably fits the text.
        data.clamp_scroll_offset(1000.0, 100.0);
        assert_eq!(data.scroll_offset, 0.0);
    }

    #[test]
    fn single_line_scrolls_to_follow_caret() {
        let text = "the quick brown fox jumps over the lazy dog repeatedly and at length";
        let mut data = make_input(false, text);
        let content_box_width = 40.0;
        let content_box_height = 20.0;

        // Caret at the end of a string that overflows a narrow input should scroll right.
        data.editor
            .driver(&mut FontContext::new(), &mut LayoutContext::new())
            .move_to_text_end();
        data.clamp_scroll_offset(content_box_width, content_box_height);

        let layout_width = data.editor.try_layout().unwrap().full_width();
        if layout_width > content_box_width {
            assert!(
                data.scroll_offset > 0.0,
                "expected horizontal scroll for overflowing single-line input"
            );
            // The caret must be within the visible region after scrolling.
            let caret = data.editor.cursor_geometry(1.5).unwrap();
            assert!(caret.x1 as f32 <= data.scroll_offset + content_box_width + 0.5);
            assert!(caret.x0 as f32 >= data.scroll_offset - 0.5);
        }

        // Moving the caret back to the start should reset the scroll offset.
        data.editor
            .driver(&mut FontContext::new(), &mut LayoutContext::new())
            .move_to_text_start();
        data.clamp_scroll_offset(content_box_width, content_box_height);
        assert_eq!(data.scroll_offset, 0.0);
    }

    #[test]
    fn multiline_scrolls_vertically_not_horizontally() {
        let text = (0..40)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut data = make_input(true, &text);
        // Constrain the width so wrapping is well-defined.
        data.editor.set_width(Some(200.0));
        data.editor
            .driver(&mut FontContext::new(), &mut LayoutContext::new())
            .refresh_layout();

        let content_box_width = 200.0;
        let content_box_height = 30.0;

        data.editor
            .driver(&mut FontContext::new(), &mut LayoutContext::new())
            .move_to_text_end();
        data.clamp_scroll_offset(content_box_width, content_box_height);

        let layout_height = data.editor.try_layout().unwrap().height();
        if layout_height > content_box_height {
            assert!(
                data.scroll_offset > 0.0,
                "expected vertical scroll for overflowing multi-line input"
            );
        }
    }

    #[test]
    fn scroll_by_clamps_and_bubbles() {
        let text = (0..40)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut data = make_input(true, &text);
        data.editor.set_width(Some(200.0));
        data.editor
            .driver(&mut FontContext::new(), &mut LayoutContext::new())
            .refresh_layout();

        let content_box_width = 200.0;
        let content_box_height = 30.0;
        let max = data.max_scroll_offset(content_box_width, content_box_height);
        assert!(max > 0.0, "test text should overflow the content box");

        // Scrolling up (positive delta decreases offset) while already at the top is a no-op and
        // the whole delta bubbles.
        assert_eq!(data.scroll_offset, 0.0);
        let bubbled = data.scroll_by(15.0, content_box_width, content_box_height);
        assert_eq!(data.scroll_offset, 0.0);
        assert_eq!(bubbled, 15.0);

        // Scrolling down moves the offset and consumes the delta.
        let bubbled = data.scroll_by(-10.0, content_box_width, content_box_height);
        assert_eq!(data.scroll_offset, 10.0);
        assert_eq!(bubbled, 0.0);

        // Scrolling past the end clamps to the maximum and bubbles the remainder. Starting at
        // offset 10 with max headroom of `max - 10`, a delta of `-(max + 100)` consumes
        // `max - 10` and bubbles the rest (`-110`).
        let bubbled = data.scroll_by(-(max + 100.0), content_box_width, content_box_height);
        assert_eq!(data.scroll_offset, max);
        assert!((bubbled - (-110.0)).abs() < 1e-3);
    }

    #[test]
    fn single_line_does_not_scroll_when_text_fits() {
        let mut data = make_input(false, "hi");
        // Wide content box; nothing to scroll, so all delta bubbles.
        let bubbled = data.scroll_by(-50.0, 1000.0, 100.0);
        assert_eq!(data.scroll_offset, 0.0);
        assert_eq!(bubbled, -50.0);
    }
}
