//! `position: fixed` must not resolve against the nearest positioned ancestor.
//!
//! Taffy has no `Fixed` position, so `stylo_taffy` maps it to `Absolute`. Laid
//! out in place, a fixed node resolved its insets against its nearest positioned
//! ancestor, so its offset was wrong and — when opposite insets were both set —
//! so was its size. Fixed nodes are now reparented onto the root element.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const VIEWPORT: (u32, u32) = (1000, 700);

fn document(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(
                VIEWPORT.0,
                VIEWPORT.1,
                1.0,
                ColorScheme::Light,
            )),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

#[track_caller]
fn assert_box(doc: &HtmlDocument, id: &str, expected: (f32, f32, f32, f32)) {
    let node_id = doc
        .get_element_by_id(id)
        .unwrap_or_else(|| panic!("no element with id {id}"));
    let node = &doc.tree()[node_id];
    let position = node.absolute_position(0.0, 0.0);
    let size = node.final_layout().size;

    let actual = (position.x, position.y, size.width, size.height);
    assert_eq!(
        actual, expected,
        "#{id}: expected x,y,w,h {expected:?} but got {actual:?}"
    );
}

const HTML: &str = r#"<html><body style="margin:0">
    <div id="free" style="position:fixed;top:10px;left:10px;width:120px;height:40px"></div>
    <div id="ancestor" style="position:relative;left:200px;top:150px;width:300px;height:200px">
        <div id="nested" style="position:fixed;top:10px;left:10px;width:120px;height:40px"></div>
    </div>
</body></html>"#;

#[test]
fn fixed_nodes_ignore_a_positioned_ancestor() {
    let doc = document(HTML);

    // Already correct: with no positioned ancestor, `absolute` and `fixed`
    // resolve against the same box.
    assert_box(&doc, "free", (10.0, 10.0, 120.0, 40.0));

    // Was offset by the ancestor's origin, landing at 210,160.
    assert_box(&doc, "nested", (10.0, 10.0, 120.0, 40.0));
}

#[test]
fn opposite_insets_size_against_the_root_rather_than_the_ancestor() {
    let doc = document(
        r#"<html><body style="margin:0">
        <div id="ancestor" style="position:relative;left:200px;top:150px;width:300px;height:200px">
            <div id="stretch" style="position:fixed;inset:0"></div>
        </div>
    </body></html>"#,
    );

    let node_id = doc.get_element_by_id("stretch").unwrap();
    let node = &doc.tree()[node_id];
    let position = node.absolute_position(0.0, 0.0);

    // Was 200,150 sized 300x200 — the ancestor's box.
    assert_eq!((position.x, position.y), (0.0, 0.0));
    assert_eq!(node.final_layout().size.width, VIEWPORT.0 as f32);
}

/// The root element is the layout root and takes its height from its content,
/// so opposite insets still size against that rather than against the viewport.
/// A browser resolves them against the initial containing block, which is always
/// viewport-sized regardless of how tall or short the document is. Blitz has no
/// separate ICB node, so this cannot be expressed by reparenting alone.
#[test]
#[ignore = "requires an initial containing block distinct from the root element"]
fn opposite_insets_should_size_against_the_viewport() {
    let doc = document(
        r#"<html><body style="margin:0">
        <div id="short" style="height:20px"></div>
        <div id="stretch" style="position:fixed;inset:0"></div>
    </body></html>"#,
    );

    assert_box(
        &doc,
        "stretch",
        (0.0, 0.0, VIEWPORT.0 as f32, VIEWPORT.1 as f32),
    );
}

#[test]
fn hoisting_survives_reconstruction() {
    let mut doc = document(HTML);

    // Full reconstruction rebuilds `layout_children` from the DOM tree, undoing
    // the reparent, so it has to be reapplied on every pass.
    doc.set_incremental_layout(false);
    for _ in 0..5 {
        doc.resolve(0.0);
    }
    assert_box(&doc, "nested", (10.0, 10.0, 120.0, 40.0));

    // The incremental path preserves `layout_children` rather than rebuilding
    // it, so reapplying the hoist has to be idempotent.
    doc.set_incremental_layout(true);
    for _ in 0..5 {
        doc.resolve(0.0);
    }
    assert_box(&doc, "nested", (10.0, 10.0, 120.0, 40.0));
}

#[test]
fn a_transformed_ancestor_keeps_its_fixed_descendants() {
    // css-transforms-1: a transformed element is the containing block for its
    // fixed descendants, so this one must be left where it is. Transforms are
    // applied at paint time and do not move layout boxes, so the check is on
    // the layout parent rather than on the resolved position.
    let doc = document(
        r#"<html><body style="margin:0">
        <div id="ancestor" style="transform:translate(200px,150px);width:300px;height:200px">
            <div id="nested" style="position:fixed;top:10px;left:10px;width:120px;height:40px"></div>
        </div>
    </body></html>"#,
    );

    let ancestor = doc.get_element_by_id("ancestor").unwrap();
    let nested = doc.get_element_by_id("nested").unwrap();

    assert_eq!(
        doc.tree()[nested].layout_parent.get(),
        Some(ancestor),
        "a fixed node under a transformed ancestor must not be hoisted"
    );
}
