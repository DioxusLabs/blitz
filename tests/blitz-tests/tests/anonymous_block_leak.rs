//! Anonymous blocks created during layout construction must be deallocated
//! once they are no longer needed.
//!
//! Regression test: a block container with mixed inline/block children wraps
//! its inline content in an anonymous block. Every time the container is
//! reconstructed a fresh anonymous block was created without freeing the
//! previous one, leaking a slab entry per reconstruction.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

// A block container ("outer") whose children are a bare text node and a block
// element. The text node must be wrapped in an anonymous block.
const HTML: &str = r#"<html><body style="margin:0">
    <div id="outer" style="width:300px;">
        some bare text
        <div id="block" style="height:50px;"></div>
    </div>
</body></html>"#;

fn count_anonymous_blocks(doc: &HtmlDocument) -> usize {
    doc.tree()
        .iter()
        .filter(|(_, node)| node.is_anonymous())
        .count()
}

#[test]
fn anonymous_blocks_do_not_leak_across_reconstructions() {
    let mut doc = HtmlDocument::from_html(
        HTML,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );

    // Force full reconstruction on every resolve so that each pass rebuilds
    // (and previously leaked) the anonymous blocks.
    doc.set_incremental_layout(false);

    doc.resolve(0.0);

    let anon_after_first = count_anonymous_blocks(&doc);
    let nodes_after_first = doc.tree().len();

    // Sanity check: our test HTML must actually generate an anonymous block,
    // otherwise the test proves nothing.
    assert!(
        anon_after_first >= 1,
        "expected the mixed inline/block container to generate an anonymous block"
    );

    // Reconstruct many times. Without deallocating stale anonymous blocks the
    // slab would grow unbounded.
    for _ in 0..20 {
        doc.resolve(0.0);
    }

    let anon_after_many = count_anonymous_blocks(&doc);
    let nodes_after_many = doc.tree().len();

    assert_eq!(
        anon_after_many, anon_after_first,
        "anonymous block count grew across reconstructions ({anon_after_first} -> \
         {anon_after_many}): stale anonymous blocks are leaking"
    );
    assert_eq!(
        nodes_after_many, nodes_after_first,
        "total node count grew across reconstructions ({nodes_after_first} -> \
         {nodes_after_many}): nodes are leaking"
    );
}

#[test]
fn anonymous_blocks_are_freed_when_owner_is_removed_from_dom() {
    let mut doc = HtmlDocument::from_html(
        HTML,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );

    doc.resolve(0.0);

    let outer_id = doc
        .query_selector("#outer")
        .unwrap()
        .expect("#outer not found");

    // The anonymous blocks owned by #outer.
    let anon_ids = doc.tree().get(outer_id).unwrap().anonymous_blocks.clone();
    assert!(
        !anon_ids.is_empty(),
        "expected #outer to own at least one anonymous block"
    );

    // Removing and dropping #outer must deallocate the anonymous blocks it owns.
    doc.mutate().remove_and_drop_node(outer_id);

    for anon_id in anon_ids {
        assert!(
            doc.tree().get(anon_id).is_none(),
            "anonymous block {anon_id} leaked after its owner was removed from the DOM"
        );
    }

    assert_eq!(
        count_anonymous_blocks(&doc),
        0,
        "no anonymous blocks should remain after removing their only owner"
    );
}
