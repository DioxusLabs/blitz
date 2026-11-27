//! A node that is detached from the DOM must not keep a `layout_parent` that
//! points at a freed (or reused) slab slot.
//!
//! Regression test: an inline element next to a block element is laid out
//! inside an anonymous block. Detaching the inline element with
//! `remove_node` keeps it alive, but the next resolve reconstructs its old
//! parent and frees the anonymous block. The detached node is not collected
//! again, so its `layout_parent` kept pointing at the freed slot. Any walk up
//! the layout tree (for example `absolute_position`, used by `getBoundingClientRect`)
//! then indexed a missing slab entry and panicked, or silently read an
//! unrelated node that had reused the slot.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

// "#inline" sits beside a block, so it is wrapped in an anonymous block.
const HTML: &str = r#"<html><body style="margin:0">
    <div id="outer" style="width:300px;">
        some bare text
        <span id="inline">inline</span>
        <div id="block" style="height:50px;"></div>
    </div>
</body></html>"#;

fn doc() -> HtmlDocument {
    HtmlDocument::from_html(
        HTML,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    )
}

#[test]
fn detached_node_does_not_keep_a_freed_layout_parent() {
    let mut doc = doc();
    doc.resolve(0.0);

    let inline_id = doc
        .query_selector("#inline")
        .unwrap()
        .expect("#inline not found");

    // Sanity check: the test needs #inline to be laid out by an anonymous
    // block, otherwise it proves nothing.
    let anon_id = doc.tree()[inline_id]
        .layout_parent
        .get()
        .expect("#inline has no layout parent");
    assert!(
        doc.tree()[anon_id].is_anonymous(),
        "expected #inline to be laid out inside an anonymous block"
    );

    // Detach #inline but keep it alive, as script does with removeChild.
    doc.mutate().remove_node(inline_id);
    doc.resolve(0.0);

    // The old anonymous block is gone (or its slot now holds another node).
    // Either way #inline must not still claim it as its layout parent.
    let layout_parent = doc.tree()[inline_id].layout_parent.get();
    if let Some(parent_id) = layout_parent {
        let parent_lays_out_inline = doc.tree().get(parent_id).is_some_and(|parent| {
            parent
                .layout_children
                .borrow()
                .as_ref()
                .is_some_and(|children| children.contains(&inline_id))
        });
        assert!(
            parent_lays_out_inline,
            "detached #inline still has layout_parent {parent_id}, \
             which does not lay it out (freed or reused slot)"
        );
    }

    // The walk that getBoundingClientRect does must not panic.
    let _ = doc.tree()[inline_id].absolute_position(0.0, 0.0);
}
