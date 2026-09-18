//! `flex-wrap: balance` and `flex-line-count` (CSS Flexbox Level 2).
//!
//! <https://drafts.csswg.org/css-flexbox-2/#flex-wrap-property>
//! <https://drafts.csswg.org/css-flexbox-2/#flex-line-count-property>

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn layout_doc(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn item_positions(doc: &HtmlDocument, count: usize) -> Vec<(f32, f32)> {
    (1..=count)
        .map(|i| {
            let id = doc
                .query_selector(&format!("#item{i}"))
                .unwrap()
                .unwrap_or_else(|| panic!("#item{i} not found"));
            let layout = doc.get_node(id).unwrap().final_layout();
            (layout.location.x, layout.location.y)
        })
        .collect()
}

fn container_html(flex_style: &str) -> String {
    format!(
        r#"<html><body style="margin:0">
            <div style="display:flex; {flex_style}; width:120px; gap:10px;">
                <div id="item1" style="width:25px; height:20px;"></div>
                <div id="item2" style="width:25px; height:20px;"></div>
                <div id="item3" style="width:25px; height:20px;"></div>
                <div id="item4" style="width:25px; height:20px;"></div>
            </div>
        </body></html>"#
    )
}

#[test]
fn flex_wrap_wrap_splits_3_1() {
    // Four 25px items with 10px gaps in a 120px container: plain `wrap`
    // fits 3 on the first line (25*3 + 10*2 = 95 <= 120) and 1 on the second.
    let doc = layout_doc(&container_html("flex-wrap: wrap"));
    let pos = item_positions(&doc, 4);
    let first_line_y = pos[0].1;
    let lines: Vec<bool> = pos.iter().map(|p| p.1 == first_line_y).collect();
    assert_eq!(
        lines,
        vec![true, true, true, false],
        "flex-wrap: wrap should split 3/1, got positions {pos:?}"
    );
}

#[test]
fn flex_wrap_balance_splits_2_2() {
    let doc = layout_doc(&container_html("flex-wrap: balance"));
    let pos = item_positions(&doc, 4);
    let first_line_y = pos[0].1;
    let lines: Vec<bool> = pos.iter().map(|p| p.1 == first_line_y).collect();
    assert_eq!(
        lines,
        vec![true, true, false, false],
        "flex-wrap: balance should split 2/2, got positions {pos:?}"
    );
}

#[test]
fn flex_wrap_wrap_balance_splits_2_2() {
    let doc = layout_doc(&container_html("flex-wrap: wrap balance"));
    let pos = item_positions(&doc, 4);
    let first_line_y = pos[0].1;
    let lines: Vec<bool> = pos.iter().map(|p| p.1 == first_line_y).collect();
    assert_eq!(lines, vec![true, true, false, false]);
}

#[test]
fn flex_wrap_wrap_reverse_balance() {
    // With wrap-reverse the balanced lines are stacked in reverse order:
    // items 1-2 end up on the bottom line, items 3-4 on the top line.
    let doc = layout_doc(&container_html("flex-wrap: wrap-reverse balance"));
    let pos = item_positions(&doc, 4);
    assert_eq!(pos[0].1, pos[1].1);
    assert_eq!(pos[2].1, pos[3].1);
    assert!(
        pos[0].1 > pos[2].1,
        "wrap-reverse balance should place the first line below the second, got {pos:?}"
    );
}

#[test]
fn flex_wrap_nowrap_balance_is_invalid() {
    // `nowrap balance` is invalid and must be ignored, leaving the initial
    // value (nowrap): all items on a single line.
    let doc = layout_doc(&container_html("flex-wrap: nowrap balance"));
    let pos = item_positions(&doc, 4);
    let first_line_y = pos[0].1;
    assert!(
        pos.iter().all(|p| p.1 == first_line_y),
        "invalid `nowrap balance` should fall back to nowrap, got {pos:?}"
    );
}

#[test]
fn flex_line_count_forces_minimum_lines() {
    // All four items would fit on one line in a 400px container, but
    // `flex-line-count: 2` with balance forces balancing across 2 lines.
    let doc = layout_doc(
        r#"<html><body style="margin:0">
            <div style="display:flex; flex-wrap: balance; flex-line-count: 2; width:400px; gap:10px;">
                <div id="item1" style="width:25px; height:20px;"></div>
                <div id="item2" style="width:25px; height:20px;"></div>
                <div id="item3" style="width:25px; height:20px;"></div>
                <div id="item4" style="width:25px; height:20px;"></div>
            </div>
        </body></html>"#,
    );
    let pos = item_positions(&doc, 4);
    let first_line_y = pos[0].1;
    let lines: Vec<bool> = pos.iter().map(|p| p.1 == first_line_y).collect();
    assert_eq!(
        lines,
        vec![true, true, false, false],
        "flex-line-count: 2 should balance into two lines, got {pos:?}"
    );
}
