//! Atomic inline-level boxes (e.g. `display: inline-flex`) laid out as inline
//! boxes within an inline formatting context must retain the scrollable
//! overflow rect computed by their own layout algorithm, so that `scrollWidth`
//! / `scrollHeight` report their overflowing content.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

#[test]
fn inline_flex_scroller_reports_overflowing_items() {
    let html = r#"<html><body style="margin:0">
<style>
.container { width: 100px; height: 100px; overflow: scroll; border: solid 3px; padding: 10px; gap: 10px; align-items: start; }
.item { min-width: 110px; min-height: 110px; }
</style>
<div id="inline" class="container" style="display: inline-flex"><div class="item"></div><div class="item"></div><div class="item"></div></div>
<div id="block" class="container" style="display: flex"><div class="item"></div><div class="item"></div><div class="item"></div></div>
    </body></html>"#;

    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);

    let scroll_size = |sel: &str| {
        let id = doc.query_selector(sel).unwrap().expect(sel);
        let node = doc.get_node(id).unwrap();
        (node.scroll_width(), node.scroll_height())
    };

    // 3 * 110px items + 2 * 10px gaps + 2 * 10px padding
    assert_eq!(scroll_size("#block"), (370.0, 130.0));
    assert_eq!(scroll_size("#inline"), scroll_size("#block"));
}
