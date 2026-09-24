//! Incremental layout must invalidate the Taffy caches (and cached
//! `content_widths`) of anonymous blocks. Anonymous blocks only appear in
//! their container's `layout_children`, never in the DOM `children`, so the
//! damage pass must reach them via `layout_parent` rather than the DOM walk.
//!
//! All style changes here are driven by `:hover` (a pure restyle) rather than
//! attribute mutation, because every mutation path inserts `CONSTRUCT_BOX`
//! damage which reconstructs (and so re-mints) anonymous blocks, masking the
//! cache-invalidation bug.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::node_id::NodeId;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn make_doc(html: &str, incremental: bool) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 400, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.set_incremental_layout(incremental);
    doc.resolve(0.0);
    doc
}

fn id(doc: &HtmlDocument, selector: &str) -> NodeId {
    doc.query_selector(selector).unwrap().unwrap()
}

fn y(doc: &HtmlDocument, selector: &str) -> f32 {
    doc.get_node(id(doc, selector))
        .unwrap()
        .final_layout()
        .location
        .y
}

fn w(doc: &HtmlDocument, selector: &str) -> f32 {
    doc.get_node(id(doc, selector))
        .unwrap()
        .final_layout()
        .size
        .width
}

fn h(doc: &HtmlDocument, selector: &str) -> f32 {
    doc.get_node(id(doc, selector))
        .unwrap()
        .final_layout()
        .size
        .height
}

/// Hover (a pure restyle) the node at `(x, y)` and re-resolve.
fn hover(doc: &mut HtmlDocument, x: f32, y: f32, expected: &str, incremental: bool) {
    let changed = doc.set_hover_to(x, y);
    assert!(
        changed,
        "hover should land on {expected} (incremental={incremental})"
    );
    assert_eq!(
        doc.get_hover_node_id(),
        Some(id(doc, expected)),
        "hover target (incremental={incremental})"
    );
    doc.resolve(0.0);
}

/// Hovering `#ib` changes only its height: a RELAYOUT-only style change on
/// an inline-block inside the anonymous block wrapping "text ... more".
///
/// The baseline-aligned inline-block's line also includes the strut's descent,
/// which depends on the system font, so heights are asserted relative to the
/// initial layout.
const SIMPLE: &str = r#"<html><head><style>
    #ib { display:inline-block; width:50px; height:20px; }
    #ib:hover { height:60px; }
</style></head><body style="margin:0; font-size:10px; line-height:1">
    <div id="c" style="width:400px;">
        text <span id="ib"></span> more
        <div id="blk" style="height:10px"></div>
    </div>
</body></html>"#;

#[test]
fn inline_block_resize_inside_anonymous_block() {
    for incremental in [false, true] {
        let mut doc = make_doc(SIMPLE, incremental);
        let blk_y = y(&doc, "#blk");
        assert!(blk_y >= 20.0, "incremental={incremental}");
        assert_eq!(h(&doc, "#c"), blk_y + 10.0, "incremental={incremental}");

        hover(&mut doc, 60.0, 10.0, "#ib", incremental);
        assert_eq!(y(&doc, "#blk"), blk_y + 40.0, "incremental={incremental}");
        assert_eq!(h(&doc, "#c"), blk_y + 50.0, "incremental={incremental}");
    }
}

/// Multi-hop `layout_parent` walk: `#ib` is in an anonymous block inside a
/// flex item (`#item`), which is itself wrapped by an anonymous block inside
/// the block container `#c` (because `#c` also has inline content). Both
/// anonymous blocks must be invalidated for `#blk` to move.
const NESTED: &str = r#"<html><head><style>
    #ib { display:inline-block; width:50px; height:20px; }
    #ib:hover { height:60px; }
    #flex { display:flex; }
    #item { width:200px; }
</style></head><body style="margin:0; font-size:10px; line-height:1">
    <div id="c" style="width:400px;">
        lead
        <div id="flex">
            <div id="item">
                text <span id="ib"></span> more
                <div id="inner" style="height:10px"></div>
            </div>
        </div>
        trail
        <div id="blk" style="height:10px"></div>
    </div>
</body></html>"#;

#[test]
fn inline_block_resize_inside_nested_anonymous_blocks() {
    for incremental in [false, true] {
        let mut doc = make_doc(NESTED, incremental);
        // lead(10) + item(line + 10) + trail(10)
        let item_h = h(&doc, "#item");
        assert!(item_h >= 30.0, "incremental={incremental}");
        assert_eq!(h(&doc, "#flex"), item_h, "incremental={incremental}");
        assert_eq!(y(&doc, "#blk"), item_h + 20.0, "incremental={incremental}");
        assert_eq!(h(&doc, "#c"), item_h + 30.0, "incremental={incremental}");

        hover(&mut doc, 60.0, 20.0, "#ib", incremental);
        assert_eq!(h(&doc, "#item"), item_h + 40.0, "incremental={incremental}");
        assert_eq!(h(&doc, "#flex"), item_h + 40.0, "incremental={incremental}");
        assert_eq!(y(&doc, "#blk"), item_h + 60.0, "incremental={incremental}");
        assert_eq!(h(&doc, "#c"), item_h + 70.0, "incremental={incremental}");
    }
}

/// `content_widths` invalidation: `#ib` sits in an anonymous block inside a
/// `width:max-content` container, so the container's intrinsic width must
/// change when `#ib` gets wider (the anonymous block's cached
/// `content_widths` would otherwise pin the old width).
const MAX_CONTENT: &str = r#"<html><head><style>
    #ib { display:inline-block; width:50px; height:10px; }
    #ib:hover { width:150px; }
    #c { width:max-content; }
</style></head><body style="margin:0; font-size:10px; line-height:1">
    <div id="c">
        <span id="ib"></span>
        <div id="blk" style="height:10px"></div>
    </div>
</body></html>"#;

#[test]
fn inline_block_resize_updates_container_max_content_width() {
    for incremental in [false, true] {
        let mut doc = make_doc(MAX_CONTENT, incremental);
        assert_eq!(w(&doc, "#c"), 50.0, "incremental={incremental}");
        assert_eq!(w(&doc, "#blk"), 50.0, "incremental={incremental}");

        hover(&mut doc, 25.0, 5.0, "#ib", incremental);
        assert_eq!(w(&doc, "#c"), 150.0, "incremental={incremental}");
        assert_eq!(w(&doc, "#blk"), 150.0, "incremental={incremental}");
    }
}
