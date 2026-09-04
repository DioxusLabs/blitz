//! Styles must be invalidated when the only change since the last re-render
//! is a `set_style_property` call.
//!
//! Regression test: `mark_style_attr_updated` set the `dirty_descendants` bit
//! on the mutated element itself (in addition to marking its ancestors dirty).
//! If the element's restyle produced no damage in its subtree,
//! `clear_damage_and_dirty_flags` (which only descends into damaged subtrees)
//! never cleared that bit. The stale bit broke the early-out invariant of
//! `Node::mark_ancestors_dirty` (a set bit implies all ancestors are set):
//! a subsequent `set_style_property` on a descendant stopped propagating at
//! the stale bit, the root was never marked dirty, and the style traversal
//! was skipped entirely.

use blitz_dom::{BaseDocument, DocumentConfig};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const HTML: &str = r#"<!DOCTYPE html>
<html><head><style>
    body { margin: 0; }
    #outer { color: rgb(0, 0, 0); }
    #inner { color: rgb(0, 0, 0); }
</style></head>
<body>
    <div id="outer">
        <div id="inner">inner text</div>
    </div>
</body></html>
"#;

fn make_doc() -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        HTML,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn color_of(doc: &BaseDocument, selector: &str) -> [u8; 3] {
    let node_id = doc.query_selector(selector).unwrap().unwrap();
    let node = doc.get_node(node_id).unwrap();
    let styles = node.primary_styles().unwrap();
    let color = styles.clone_color().into_srgb_legacy();
    let srgb = color.raw_components();
    [
        (srgb[0] * 255.0).round() as u8,
        (srgb[1] * 255.0).round() as u8,
        (srgb[2] * 255.0).round() as u8,
    ]
}

/// A lone `set_style_property` call between two renders must restyle the node.
#[test]
fn set_style_property_restyles_node() {
    let mut doc = make_doc();
    let inner_id = doc.query_selector("#inner").unwrap().unwrap();

    doc.set_style_property(inner_id, "color", "rgb(255, 0, 0)");
    doc.resolve(0.0);
    assert_eq!(color_of(&doc, "#inner"), [255, 0, 0]);
}

/// A `set_style_property` call on an element inside a `display: none` subtree
/// must not leave a stale `dirty_descendants` bit behind. The style traversal
/// never visits such elements (and so never clears their flags); a stale bit
/// makes `mark_ancestors_dirty` early-out on a later mutation of a descendant,
/// skipping the style traversal entirely.
#[test]
fn set_style_property_inside_display_none_subtree() {
    let mut doc = make_doc();
    let outer_id = doc.query_selector("#outer").unwrap().unwrap();
    let inner_id = doc.query_selector("#inner").unwrap().unwrap();

    doc.set_style_property(outer_id, "display", "none");
    doc.resolve(0.0);

    // #inner is display:none and unstyled; this sets its dirty_descendants
    // flag, which the traversal never clears.
    doc.set_style_property(inner_id, "color", "rgb(0, 255, 0)");
    doc.resolve(0.0);

    doc.set_style_property(outer_id, "display", "block");
    doc.resolve(0.0);
    assert_eq!(color_of(&doc, "#inner"), [0, 255, 0]);

    // Mutating a node below the (potentially stale) #inner bit must still
    // trigger a restyle.
    doc.set_style_property(inner_id, "color", "rgb(255, 0, 0)");
    doc.resolve(0.0);
    assert_eq!(color_of(&doc, "#inner"), [255, 0, 0]);
}

/// A `set_style_property` call which does not change the element's computed
/// style must not prevent a later `set_style_property` on a descendant from
/// being picked up.
#[test]
fn set_style_property_after_undamaged_style_attr_update() {
    let mut doc = make_doc();
    let outer_id = doc.query_selector("#outer").unwrap().unwrap();
    let inner_id = doc.query_selector("#inner").unwrap().unwrap();

    // Set a style property on #outer whose value matches its existing
    // computed style: the style attribute changes but the restyle produces
    // no damage in the #outer subtree.
    doc.set_style_property(outer_id, "color", "rgb(0, 0, 0)");
    doc.resolve(0.0);
    assert_eq!(color_of(&doc, "#outer"), [0, 0, 0]);

    // Now change a descendant. The restyle must still run.
    doc.set_style_property(inner_id, "color", "rgb(255, 0, 0)");
    doc.resolve(0.0);
    assert_eq!(
        color_of(&doc, "#inner"),
        [255, 0, 0],
        "style traversal was skipped due to a stale dirty_descendants flag"
    );
}
