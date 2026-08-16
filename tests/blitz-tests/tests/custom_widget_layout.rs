//! Layout of replaced elements that carry a custom widget.
//!
//! Regression test for #706: `set_custom_widget` writes into the same
//! `SpecialElementData` slot the replaced-layout match reads, so any replaced
//! element carrying a widget used to reach `unreachable!()` on the next layout.

use blitz_dom::{DocumentConfig, Widget};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

struct Probe;
impl Widget for Probe {}

struct SizedProbe;
impl Widget for SizedProbe {
    fn intrinsic_size(&self) -> Option<taffy::Size<f32>> {
        Some(taffy::Size {
            width: 64.0,
            height: 32.0,
        })
    }

    fn aspect_ratio(&self) -> Option<f32> {
        Some(2.0)
    }
}

fn widget_size(html: &str, widget: Box<dyn Widget>) -> (f32, f32) {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );

    let node_id = doc
        .query_selector("#widget")
        .unwrap()
        .expect("#widget not found");
    doc.mutate().set_custom_widget(node_id, widget);
    doc.resolve(0.0);

    let layout = doc.get_node(node_id).unwrap().final_layout();
    (layout.size.width, layout.size.height)
}

#[test]
fn canvas_with_widget_keeps_its_attribute_size() {
    let size = widget_size(
        r#"<html><body style="margin:0;">
            <canvas id="widget" width="200" height="100"></canvas>
        </body></html>"#,
        Box::new(Probe),
    );
    assert_eq!(size, (200.0, 100.0));
}

#[test]
fn canvas_with_widget_keeps_its_intrinsic_ratio() {
    let size = widget_size(
        r#"<html><body style="margin:0;">
            <canvas id="widget" width="200" height="100" style="width: 50px;"></canvas>
        </body></html>"#,
        Box::new(Probe),
    );
    assert_eq!(size, (50.0, 25.0));
}

#[test]
fn widget_without_attributes_uses_the_default_object_size() {
    let size = widget_size(
        r#"<html><body style="margin:0;">
            <video id="widget"></video>
        </body></html>"#,
        Box::new(Probe),
    );
    assert_eq!(size, (300.0, 150.0));
}

#[test]
fn widget_reported_size_is_the_intrinsic_size() {
    let size = widget_size(
        r#"<html><body style="margin:0;">
            <video id="widget"></video>
        </body></html>"#,
        Box::new(SizedProbe),
    );
    assert_eq!(size, (64.0, 32.0));
}

#[test]
fn widget_reported_ratio_carries_the_cross_axis() {
    let size = widget_size(
        r#"<html><body style="margin:0;">
            <video id="widget" style="width: 100px;"></video>
        </body></html>"#,
        Box::new(SizedProbe),
    );
    assert_eq!(size, (100.0, 50.0));
}

#[test]
fn content_attributes_override_the_widget_reported_size() {
    let size = widget_size(
        r#"<html><body style="margin:0;">
            <canvas id="widget" width="120" height="60"></canvas>
        </body></html>"#,
        Box::new(SizedProbe),
    );
    assert_eq!(size, (120.0, 60.0));
}
