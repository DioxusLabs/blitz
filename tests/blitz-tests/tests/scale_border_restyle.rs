use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use blitz_traits::shell::{ColorScheme, Viewport};

fn document(scale: f32, zoom: bool) -> HtmlDocument {
    let mut viewport = Viewport::new(800, 600, 1.0, ColorScheme::Light);
    if zoom {
        viewport.set_zoom(scale);
    } else {
        viewport.set_hidpi_scale(scale);
    }
    HtmlDocument::from_html(
        r#"<!DOCTYPE html><style>
            #box { width: 100px; height: 40px; border: 1px solid; }
            #child { border: 0.7px solid; }
        </style><div id="box"><div id="child"></div></div>"#,
        DocumentConfig {
            viewport: Some(viewport),
            ..Default::default()
        },
    )
}

fn borders(doc: &HtmlDocument, selector: &str) -> [f32; 4] {
    let id = doc.query_selector(selector).unwrap().unwrap();
    let node = doc.get_node(id).unwrap();
    let style = node.primary_styles().unwrap();
    let border = style.get_border();
    [
        border.border_top_width.0.to_f32_px(),
        border.border_right_width.0.to_f32_px(),
        border.border_bottom_width.0.to_f32_px(),
        border.border_left_width.0.to_f32_px(),
    ]
}

#[test]
fn changed_scale_matches_initial_scale_border_snapping() {
    for zoom in [false, true] {
        for (initial, target) in [(1.0, 1.5), (1.5, 1.0)] {
            let mut changed = document(initial, zoom);
            changed.resolve(0.0);
            let before = borders(&changed, "#box");
            if zoom {
                changed.viewport_mut().set_zoom(target);
            } else {
                changed.viewport_mut().set_hidpi_scale(target);
            }
            changed.resolve(0.0);

            let mut fresh = document(target, zoom);
            fresh.resolve(0.0);
            assert_ne!(before, borders(&fresh, "#box"));
            for selector in ["#box", "#child"] {
                assert_eq!(borders(&changed, selector), borders(&fresh, selector));
            }
        }
    }
}
