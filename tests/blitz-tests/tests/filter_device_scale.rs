//! CSS filter lengths and the geometry passed to the backend use the same
//! device-pixel coordinate space. Unitless filter parameters must not scale.
use anyrender::filters::FilterEffect;
use anyrender::recording::{RenderCommand, Scene};
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn effects(property: &str, scale: f32) -> Vec<FilterEffect> {
    let html = format!(
        "<html><body style='margin:0'><div style='margin:40px;width:80px;height:60px;\
         background:red;{property}:blur(8px) drop-shadow(6px -4px 2px blue) brightness(.7)'>\
         </div></body></html>"
    );
    let size = (200.0 * scale) as u32;
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(size, size, scale, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let mut scene = Scene::new();
    paint_scene(&mut scene, doc.as_mut(), scale as f64, size, size, 0, 0);
    let filters: Vec<_> = scene
        .commands
        .iter()
        .filter_map(|command| {
            let RenderCommand::PushLayer(layer) = command else {
                return None;
            };
            if property == "filter" {
                layer.filter.as_ref()
            } else {
                layer.backdrop_filter.as_ref()
            }
        })
        .collect();
    assert_eq!(filters.len(), 1, "expected one {property} layer");
    filters[0]
        .nodes()
        .iter()
        .map(|node| node.effect.clone())
        .collect()
}

#[test]
fn filter_lengths_follow_the_paint_scale() {
    for property in ["filter", "backdrop-filter"] {
        let unscaled = effects(property, 1.0);
        for scale in [0.5, 1.0, 1.5, 2.0, 3.0] {
            let actual = effects(property, scale);
            assert_eq!(actual.len(), 3);
            let FilterEffect::GaussianBlur(blur) = &actual[0] else {
                panic!("expected blur")
            };
            assert_eq!(blur.std_deviation, 8.0 * scale, "{property}, scale {scale}");
            let FilterEffect::DropShadow(shadow) = &actual[1] else {
                panic!("expected shadow")
            };
            assert_eq!(
                (shadow.dx, shadow.dy, shadow.std_deviation),
                (6.0 * scale, -4.0 * scale, 2.0 * scale),
                "{property}, scale {scale}"
            );
            assert_eq!(actual[2], unscaled[2], "brightness is unitless");
        }
    }
}
