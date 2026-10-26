//! Atomic inline-level boxes (e.g. `display: inline-flex`) laid out as inline
//! boxes within an inline formatting context must retain the scrollable
//! overflow rect computed by their own layout algorithm, so that `scrollWidth`
//! / `scrollHeight` report their overflowing content.

use blitz_dom::{DocumentConfig, ScrollBehavior};
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

fn reverse_scroll_doc(style: &str, items: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        &format!(
            r#"<html><body style="margin:0">
            <div id="scroller" style="display:flex;width:100px;height:100px;overflow:auto;{style}">{items}</div>
            </body></html>"#
        ),
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

#[test]
fn reversed_flex_origins_scroll_to_negative_offsets_and_hit_the_painted_child() {
    for (style, horizontal) in [
        ("flex-wrap:wrap-reverse", false),
        ("flex-direction:column-reverse", false),
        ("flex-direction:row-reverse", true),
        ("direction:rtl", true),
        ("flex-direction:column;flex-wrap:wrap-reverse", true),
    ] {
        let items = if style.contains("wrap") {
            r#"<div id="first" style="flex:none;width:100px;height:100px"></div><div id="last" style="flex:none;width:100px;height:100px"></div>"#
        } else if horizontal {
            r#"<div id="first" style="flex:none;width:200px;height:100px"></div>"#
        } else {
            r#"<div id="first" style="flex:none;width:100px;height:200px"></div>"#
        };
        let mut doc = reverse_scroll_doc(style, items);
        let id = doc.query_selector("#scroller").unwrap().unwrap();
        let node = doc.get_node(id).unwrap();
        assert_eq!(
            (node.scroll_offset().x, node.scroll_offset().y),
            (0.0, 0.0),
            "{style}"
        );
        assert_eq!(
            if horizontal {
                node.scroll_width()
            } else {
                node.scroll_height()
            },
            200.0,
            "{style}"
        );
        doc.scroll_by(id, -1000.0, -1000.0, ScrollBehavior::Instant);
        let node = doc.get_node(id).unwrap();
        assert_eq!(
            if horizontal {
                node.scroll_offset().x
            } else {
                node.scroll_offset().y
            },
            -100.0,
            "{style}"
        );
        let expected = doc
            .query_selector(if style.contains("wrap") {
                "#last"
            } else {
                "#first"
            })
            .unwrap()
            .unwrap();
        assert_eq!(
            doc.element_from_point(50.0, 50.0),
            Some(expected),
            "{style}"
        );
        doc.scroll_by(id, 1000.0, 1000.0, ScrollBehavior::Instant);
        let node = doc.get_node(id).unwrap();
        assert_eq!(
            (node.scroll_offset().x, node.scroll_offset().y),
            (0.0, 0.0),
            "{style}"
        );
    }
}

#[test]
fn normal_origin_negative_overflow_stays_unreachable() {
    let mut doc = reverse_scroll_doc(
        "align-items:flex-end",
        r#"<div style="flex:none;width:100px;height:200px"></div>"#,
    );
    let id = doc.query_selector("#scroller").unwrap().unwrap();
    assert_eq!(doc.get_node(id).unwrap().scroll_height(), 100.0);
    doc.scroll_by(id, 0.0, -1000.0, ScrollBehavior::Instant);
    assert_eq!(doc.get_node(id).unwrap().scroll_offset().y, 0.0);
}

#[test]
fn empty_reversed_scroller_and_clipped_overflow_cannot_scroll() {
    for (style, items) in [
        ("flex-wrap:wrap-reverse", ""),
        (
            "flex-direction:column-reverse;overflow:clip",
            r#"<div style="flex:none;height:200px;width:100px"></div>"#,
        ),
    ] {
        let mut doc = reverse_scroll_doc(style, items);
        let id = doc.query_selector("#scroller").unwrap().unwrap();
        doc.scroll_by(id, -1000.0, -1000.0, ScrollBehavior::Instant);
        let node = doc.get_node(id).unwrap();
        assert_eq!((node.scroll_offset().x, node.scroll_offset().y), (0.0, 0.0));
    }
}

#[test]
fn negative_margins_do_not_extend_normal_origin_scroll_dimensions() {
    let doc = reverse_scroll_doc(
        "flex-direction:column;height:auto;padding:10px 20px;overflow:visible",
        &r#"<div style="flex:none;width:20px;height:20px;margin:-5px -7px"></div>"#.repeat(4),
    );
    let id = doc.query_selector("#scroller").unwrap().unwrap();
    let node = doc.get_node(id).unwrap();
    assert_eq!(node.client_height(), 60.0);
    assert_eq!(node.scroll_width(), node.client_width());
    assert_eq!(node.scroll_height(), node.client_height());
}

#[test]
fn sticky_bottom_tracks_a_reversed_scrollport_without_changing_its_range() {
    let mut doc = reverse_scroll_doc(
        "flex-direction:column-reverse;scrollbar-width:none",
        r#"<div id="sticky" style="position:sticky;bottom:0;flex:none;width:50px;height:50px"></div><div style="flex:none;height:400px"></div>"#,
    );
    let scroller = doc.query_selector("#scroller").unwrap().unwrap();
    let sticky = doc.query_selector("#sticky").unwrap().unwrap();
    assert_eq!(doc.get_node(sticky).unwrap().sticky_offset().y, 0.0);
    for scroll in [-100.0, -1000.0, 0.0] {
        doc.scroll_to(scroller, 0.0, scroll, ScrollBehavior::Instant);
        let node = doc.get_node(scroller).unwrap();
        let scroll = node.scroll_offset().y;
        assert_eq!(node.scroll_height(), 450.0);
        let node = doc.get_node(sticky).unwrap();
        assert_eq!(node.sticky_offset().y, scroll as f32);
        assert_eq!(node.absolute_position(0.0, 0.0).y, 50.0);
        assert_eq!(doc.element_from_point(25.0, 75.0), Some(sticky));
    }
}

#[test]
fn reversed_scrolling_without_a_sticky_inset_keeps_normal_positioning() {
    let mut doc = reverse_scroll_doc(
        "flex-direction:column-reverse",
        r#"<div id="sticky" style="position:sticky;flex:none;height:200px"></div>"#,
    );
    let scroller = doc.query_selector("#scroller").unwrap().unwrap();
    let sticky = doc.query_selector("#sticky").unwrap().unwrap();
    doc.scroll_to(scroller, 0.0, -1000.0, ScrollBehavior::Instant);
    assert_eq!(doc.get_node(sticky).unwrap().sticky_offset().y, 0.0);
}
