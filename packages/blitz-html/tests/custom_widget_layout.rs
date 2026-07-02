//! Tests for custom widget layout: widgets are laid out as leaf ("replaced") nodes,
//! sized using the values returned by `Widget::intrinsic_size` by default, and can
//! implement fully custom sizing by overriding `Widget::layout`.

use blitz_dom::{Atom, DocumentConfig, Widget, WidgetIntrinsicSize, taffy};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

/// A widget which uses the default `intrinsic_size` and `layout` implementations
struct DefaultWidget;
impl Widget for DefaultWidget {}

/// A widget with a custom intrinsic size
struct IntrinsicSizeWidget(WidgetIntrinsicSize);
impl Widget for IntrinsicSizeWidget {
    fn intrinsic_size(&mut self) -> WidgetIntrinsicSize {
        self.0
    }
}

/// A widget with a fully custom `layout` implementation
struct FixedLayoutWidget;
impl Widget for FixedLayoutWidget {
    fn layout(
        &mut self,
        _inputs: taffy::LayoutInput,
        _style: &taffy::Style<Atom>,
    ) -> taffy::LayoutOutput {
        let size = taffy::Size {
            width: 123.0,
            height: 45.0,
        };
        taffy::LayoutOutput {
            size,
            content_size: size,
            first_baselines: taffy::Point::NONE,
            top_margin: taffy::CollapsibleMarginSet::ZERO,
            bottom_margin: taffy::CollapsibleMarginSet::ZERO,
            margins_can_collapse_through: false,
        }
    }
}

/// Create a document containing a single `<object>` element (with the given styles)
/// which has the given custom widget attached, and resolve style and layout.
///
/// The `<object>` is placed in a "shrink-to-fit" context (a flex column with
/// `align-items: flex-start`) so that it is sized by its own intrinsic size rather
/// than being stretched to the width of its container.
fn widget_doc(widget: impl Widget + 'static, style: &str) -> (HtmlDocument, usize) {
    let html = format!(
        r#"<html><body style="margin:0; display:flex; flex-direction:column; align-items:flex-start">
            <object id="widget" style="{style}"></object>
        </body></html>"#
    );
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    let node_id = doc.query_selector("#widget").unwrap().expect("#widget");
    doc.mutate().set_custom_widget(node_id, Box::new(widget));
    doc.resolve(0.0);
    (doc, node_id)
}

fn layout_size(doc: &HtmlDocument, node_id: usize) -> (f32, f32) {
    let size = doc.get_node(node_id).unwrap().final_layout.size;
    (size.width, size.height)
}

#[test]
fn default_intrinsic_size_is_300_by_150() {
    let (doc, id) = widget_doc(DefaultWidget, "");
    assert_eq!(layout_size(&doc, id), (300.0, 150.0));
}

#[test]
fn css_size_scales_other_axis_via_derived_aspect_ratio() {
    // The intrinsic aspect ratio (300/150 = 2) determines the height from the CSS width
    let (doc, id) = widget_doc(DefaultWidget, "width: 600px");
    assert_eq!(layout_size(&doc, id), (600.0, 300.0));

    let (doc, id) = widget_doc(DefaultWidget, "height: 300px");
    assert_eq!(layout_size(&doc, id), (600.0, 300.0));

    // Explicit width and height take priority over the intrinsic size / aspect ratio
    let (doc, id) = widget_doc(DefaultWidget, "width: 100px; height: 100px");
    assert_eq!(layout_size(&doc, id), (100.0, 100.0));
}

#[test]
fn max_size_constraints_preserve_aspect_ratio() {
    let (doc, id) = widget_doc(DefaultWidget, "max-width: 150px");
    assert_eq!(layout_size(&doc, id), (150.0, 75.0));
}

#[test]
fn explicit_intrinsic_aspect_ratio() {
    let intrinsic = WidgetIntrinsicSize {
        width: None,
        height: None,
        aspect_ratio: Some(4.0),
    };

    // With no CSS size, the width falls back to CSS's default object size (300px)
    // and the height is derived from the aspect ratio
    let (doc, id) = widget_doc(IntrinsicSizeWidget(intrinsic), "");
    assert_eq!(layout_size(&doc, id), (300.0, 75.0));

    // With a CSS width, the height is derived from the aspect ratio
    let (doc, id) = widget_doc(IntrinsicSizeWidget(intrinsic), "width: 200px");
    assert_eq!(layout_size(&doc, id), (200.0, 50.0));
}

#[test]
fn partial_intrinsic_size_falls_back_to_default_object_size() {
    // Missing intrinsic height falls back to 150px
    let intrinsic = WidgetIntrinsicSize {
        width: Some(100.0),
        height: None,
        aspect_ratio: None,
    };
    let (doc, id) = widget_doc(IntrinsicSizeWidget(intrinsic), "");
    assert_eq!(layout_size(&doc, id), (100.0, 150.0));

    // Missing intrinsic width falls back to 300px
    let intrinsic = WidgetIntrinsicSize {
        width: None,
        height: Some(100.0),
        aspect_ratio: None,
    };
    let (doc, id) = widget_doc(IntrinsicSizeWidget(intrinsic), "");
    assert_eq!(layout_size(&doc, id), (300.0, 100.0));
}

#[test]
fn intrinsic_size_and_explicit_aspect_ratio() {
    // An explicit aspect ratio resolves a missing dimension
    let intrinsic = WidgetIntrinsicSize {
        width: Some(100.0),
        height: None,
        aspect_ratio: Some(2.0),
    };
    let (doc, id) = widget_doc(IntrinsicSizeWidget(intrinsic), "");
    assert_eq!(layout_size(&doc, id), (100.0, 50.0));
}

#[test]
fn custom_layout_implementation_is_used() {
    let (doc, id) = widget_doc(FixedLayoutWidget, "");
    assert_eq!(layout_size(&doc, id), (123.0, 45.0));

    // The custom layout implementation ignores CSS sizes entirely
    let (doc, id) = widget_doc(FixedLayoutWidget, "width: 600px");
    assert_eq!(layout_size(&doc, id), (123.0, 45.0));
}

#[test]
fn range_input_intrinsic_size() {
    // `<input type="range">` should be sized by its UA stylesheet dimensions (160x16),
    // which match the RangeInputWidget's intrinsic size
    let mut doc = HtmlDocument::from_html(
        r#"<html><body style="margin:0"><input type="range" id="slider"></body></html>"#,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let node_id = doc.query_selector("#slider").unwrap().expect("#slider");
    assert_eq!(layout_size(&doc, node_id), (160.0, 16.0));
}
