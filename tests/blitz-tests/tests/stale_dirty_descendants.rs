//! Hover invalidation must keep working after a restyle which visits a
//! subtree without producing any damage in it.
//!
//! Regression test for unreliable hover invalidation on bbc.co.uk/news:
//! during the style traversal, stylo's `note_children` sets the
//! `dirty_descendants` bit on already-visited elements (for Gecko's
//! post-traversal, which Blitz does not run). Blitz's `TElement`
//! implementation additionally re-marked all ancestors dirty, re-flagging
//! nodes whose bits had already been cleared earlier in the preorder
//! traversal. `clear_damage_and_dirty_flags` only descends into damaged
//! subtrees, so branches which were restyled without producing damage kept
//! stale `dirty_descendants` bits while their ancestors (on damaged paths)
//! were cleared.
//!
//! That broke the invariant `mark_ancestors_dirty` relies on for its
//! early-out (a set bit implies all ancestors are set): the next hover
//! change below a stale bit stopped propagating to the root, the root was
//! never considered dirty, and the style traversal was skipped entirely —
//! leaving `:hover` styles unapplied until unrelated damage happened to
//! sweep the stale bits.

use blitz_dom::{BaseDocument, DocumentConfig};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const HTML: &str = r#"<!DOCTYPE html>
<html><head><style>
    body { margin: 0; }
    a { display: block; width: 100px; height: 20px; color: rgb(0, 0, 0); }
    #branch { padding-top: 20px; }
    #menu-link:hover { color: rgb(255, 0, 0); }
    #nav-link:hover { color: rgb(255, 0, 0); }
    /* Redundant declaration: hovering #branch restyles .foo without changing
       its computed style, so the #branch subtree is visited but undamaged */
    #branch:hover .foo { color: rgb(0, 0, 0); }
</style></head>
<body>
    <div id="nav" style="height: 40px">
        <div id="branch">
            <div id="branch2">
                <span class="foo">foo</span>
            </div>
            <a id="menu-link" href="/menu">menu link</a>
        </div>
        <a id="nav-link" href="/nav">nav link</a>
    </div>
</body></html>
"#;

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

#[test]
fn hover_keeps_working_after_undamaged_subtree_restyle() {
    let mut doc = HtmlDocument::from_html(
        HTML,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);

    // Hover #nav-link.
    let nav_link_id = doc.query_selector("#nav-link").unwrap().unwrap();
    let pos = doc
        .get_node(nav_link_id)
        .unwrap()
        .absolute_position(5.0, 5.0);
    doc.set_hover_to(pos.x, pos.y);
    doc.resolve(0.0);
    assert_eq!(color_of(&doc, "#nav-link"), [255, 0, 0]);

    // Hover #branch's padding area. This restyles .foo (via `#branch:hover
    // .foo`) without changing its style (no damage in the #branch subtree),
    // while unhovering #nav-link damages the #nav-link path.
    let branch_id = doc.query_selector("#branch").unwrap().unwrap();
    let pos = doc.get_node(branch_id).unwrap().absolute_position(5.0, 5.0);
    doc.set_hover_to(pos.x, pos.y);
    doc.resolve(0.0);
    assert_eq!(color_of(&doc, "#nav-link"), [0, 0, 0]);

    // Hover #menu-link (inside #branch). The hover restyle must still run.
    let menu_link_id = doc.query_selector("#menu-link").unwrap().unwrap();
    let pos = doc
        .get_node(menu_link_id)
        .unwrap()
        .absolute_position(5.0, 5.0);
    doc.set_hover_to(pos.x, pos.y);
    doc.resolve(0.0);
    assert_eq!(
        color_of(&doc, "#menu-link"),
        [255, 0, 0],
        "hover restyle was skipped due to stale dirty_descendants flags"
    );
}
