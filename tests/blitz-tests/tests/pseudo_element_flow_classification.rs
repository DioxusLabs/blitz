//! A `::before`/`::after` pseudo-element that generates its own box takes part in the
//! inline-vs-block classification of its container like a child element does, so a
//! container whose only content is an inline-level pseudo gets an inline formatting
//! context (and a baseline from it) rather than the pseudo as a block-level child.

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

fn rect(doc: &HtmlDocument, selector: &str) -> (f32, f32) {
    let id = doc.query_selector(selector).unwrap().unwrap();
    let layout = doc.get_node(id).unwrap().final_layout();
    (layout.location.y, layout.size.height)
}

const STYLE: &str = "<style>
  body { margin: 0; font-size: 0; }
  .flex { display: flex; align-items: baseline; }
  .ref { width: 10px; height: 200px; }
  .item::before { content: ''; display: inline-block; width: 25px; height: 25px; }
</style>";

/// The pseudo is `vertical-align: top` in a zero-height line, so the line's baseline (and
/// the item's first baseline) is at the top of the line box: the item is baseline-aligned
/// with the bottom of the 200px reference box.
#[test]
fn inline_block_pseudo_only_child_exports_line_baseline() {
    let doc = layout_doc(&format!(
        r#"{STYLE}<style>#top::before {{ vertical-align: top; }}</style><div class="flex"><div class="item" id="top" style="padding-bottom: 25px"></div><div class="ref"></div></div>"#
    ));
    let (y, height) = rect(&doc, "#top");
    assert_eq!(height, 50.0);
    assert_eq!(y, 200.0);
}

/// With `vertical-align: bottom` the line box grows above the baseline instead, so the
/// item's baseline is at the bottom of its content box.
#[test]
fn inline_block_pseudo_only_child_bottom_aligned() {
    let doc = layout_doc(&format!(
        r#"{STYLE}<style>#bottom::before {{ vertical-align: bottom; }}</style><div class="flex"><div class="item" id="bottom" style="padding-top: 25px"></div><div class="ref"></div></div>"#
    ));
    let (y, height) = rect(&doc, "#bottom");
    assert_eq!(height, 50.0);
    assert_eq!(y, 150.0);
}

/// WPT `css/css-grid/layout-algorithm/grid-minimum-contribution-baseline-shim.html`: the
/// top-aligned item is shimmed down by the 50px difference in baselines, which grows the row.
#[test]
fn grid_row_grows_by_baseline_shim_of_pseudo_only_items() {
    let doc = layout_doc(&format!(
        r#"{STYLE}<style>
          .grid {{ display: grid; position: relative; width: 0; height: 0; grid-template-columns: 50px 50px; grid-template-rows: minmax(auto, auto); align-items: baseline; }}
          #area {{ position: absolute; inset: 0; grid-column: 1 / 3; grid-row: 1 / 2; }}
          #item1::before {{ vertical-align: bottom; }}
          #item2::before {{ vertical-align: top; }}
        </style>
        <div class="grid"><div class="item" id="item1" style="padding-top: 25px"></div><div class="item" id="item2" style="padding-bottom: 25px"></div><div id="area"></div></div>"#
    ));
    assert_eq!(rect(&doc, "#item1"), (0.0, 50.0));
    assert_eq!(rect(&doc, "#item2"), (50.0, 50.0));
    assert_eq!(rect(&doc, "#area"), (0.0, 100.0));
}
