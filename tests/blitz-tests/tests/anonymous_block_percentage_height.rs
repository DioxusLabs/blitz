use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

#[test]
fn anonymous_block_preserves_percentage_height_containing_block() {
    for container_height in ["200px", "auto"] {
        let html = format!(
            r#"<!DOCTYPE html><style>
                body {{ margin: 0; }}
                #flex {{ display: flex; flex-direction: column; }}
                #item {{ width: 200px; height: {container_height}; }}
                #target {{ height: 100%; }}
            </style>
            <div id="flex"><div id="item">
                <div></div><canvas id="target" width="100" height="100"></canvas>
            </div></div>"#
        );
        let mut doc = HtmlDocument::from_html(
            &html,
            DocumentConfig {
                viewport: Some(Viewport::new(400, 400, 1.0, ColorScheme::Light)),
                html_parser_provider: Some(Arc::new(HtmlProvider) as _),
                ..Default::default()
            },
        );
        doc.resolve(0.0);
        let target = doc.query_selector("#target").unwrap().unwrap();
        let size = doc.get_node(target).unwrap().final_layout().size;
        let expected = if container_height == "auto" {
            100.0
        } else {
            200.0
        };
        assert_eq!(size.width, expected, "height: {container_height}");
        assert_eq!(size.height, expected, "height: {container_height}");
    }
}
