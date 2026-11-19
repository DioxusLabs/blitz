//! Focused regression tests for box-tree invalidation when nodes detach,
//! move, are dropped, or transition into `display: none` / `display: contents`.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn make_doc(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

/// Regression test for the crash in PR #1668: an inline child wrapped in an
/// anonymous block is detached without being dropped, then its former parent
/// reconstructs and frees the anonymous block.
#[test]
fn detached_inline_in_anonymous_block_clears_layout_parent() {
    let mut doc = make_doc(
        r#"<!DOCTYPE html>
        <html><body>
            <div id="parent">
                <div id="block">block</div>
                <span id="inline">text</span>
            </div>
        </body></html>"#,
    );

    let span_id = doc.query_selector("#inline").unwrap().unwrap();
    let old_layout_parent = doc.get_node(span_id).unwrap().layout_parent.get().unwrap();
    assert!(
        doc.get_node(old_layout_parent).unwrap().is_anonymous(),
        "expected inline child next to a block sibling to sit inside an anonymous block"
    );

    doc.mutate().remove_node(span_id);
    assert_eq!(doc.get_node(span_id).unwrap().layout_parent.get(), None);

    // Reconstructing #parent frees the old anonymous block.
    doc.resolve(0.0);
    assert!(
        doc.get_node(old_layout_parent).is_none(),
        "expected old anonymous block to be deallocated"
    );
    assert_eq!(doc.get_node(span_id).unwrap().layout_parent.get(), None);
    let _ = doc.get_node(span_id).unwrap().absolute_position(0.0, 0.0);
}

/// A detached child whose *real* (non-anonymous) parent is reconstructed
/// without it and then dropped must not retain a dangling `layout_parent`.
#[test]
fn detached_child_real_parent_reconstructed_then_dropped() {
    let mut doc = make_doc(
        r#"<!DOCTYPE html>
        <html><body>
            <div id="parent"><div id="child" style="width: 50px; height: 20px;"></div></div>
        </body></html>"#,
    );

    let parent_id = doc.query_selector("#parent").unwrap().unwrap();
    let child_id = doc.query_selector("#child").unwrap().unwrap();
    assert_eq!(
        doc.get_node(child_id).unwrap().layout_parent.get(),
        Some(parent_id)
    );

    doc.mutate().remove_node(child_id);
    doc.resolve(0.0);
    doc.mutate().remove_and_drop_node(parent_id);
    doc.resolve(0.0);

    assert!(doc.get_node(parent_id).is_none());
    let child = doc.get_node(child_id).unwrap();
    assert_eq!(child.layout_parent.get(), None);
    assert_eq!(child.containing_block(), None);
    let _ = child.absolute_position(0.0, 0.0);
}

/// Detaching an out-of-flow node must immediately unlink it from its
/// containing block's `hoisted_children` and clear `oof_containing_block`,
/// even before relayout or if the containing block is subsequently dropped.
#[test]
fn detached_oof_unlinks_from_containing_block_and_survives_cb_drop() {
    let mut doc = make_doc(
        r#"<!DOCTYPE html>
        <html><body>
            <div id="cb" style="position: relative; width: 200px; height: 200px;">
                <div id="holder">
                    <div id="oof" style="position: absolute; top: 10px; left: 10px; width: 30px; height: 30px;"></div>
                </div>
            </div>
        </body></html>"#,
    );

    let cb_id = doc.query_selector("#cb").unwrap().unwrap();
    let oof_id = doc.query_selector("#oof").unwrap().unwrap();

    assert_eq!(
        doc.get_node(oof_id).unwrap().oof_containing_block.get(),
        Some(cb_id)
    );
    assert!(
        doc.get_node(cb_id)
            .unwrap()
            .hoisted_children
            .borrow()
            .contains(&oof_id)
    );

    doc.mutate().remove_node(oof_id);

    assert_eq!(
        doc.get_node(oof_id).unwrap().oof_containing_block.get(),
        None
    );
    assert!(
        !doc.get_node(cb_id)
            .unwrap()
            .hoisted_children
            .borrow()
            .contains(&oof_id)
    );

    doc.mutate().remove_and_drop_node(cb_id);
    doc.resolve(0.0);

    let oof = doc.get_node(oof_id).unwrap();
    assert_eq!(oof.oof_containing_block.get(), None);
    assert_eq!(oof.containing_block(), None);
    let _ = oof.absolute_position(0.0, 0.0);
}

/// Detaching a subtree that owns anonymous blocks and `::before`/`::after`
/// pseudo-elements must drop those layout-only nodes so reparenting a former
/// child back into the document does not collide with stale detached boxes.
#[test]
fn detached_subtree_drops_anonymous_blocks_and_pseudos_before_reparent() {
    let mut doc = make_doc(
        r#"<!DOCTYPE html>
        <html><head><style>
            #subtree::before { content: "before"; display: block; }
            #subtree::after { content: "after"; display: block; }
        </style></head><body>
            <div id="host"></div>
            <div id="subtree">
                <div id="block">block</div>
                <span id="inline">text</span>
            </div>
        </body></html>"#,
    );

    let host_id = doc.query_selector("#host").unwrap().unwrap();
    let subtree_id = doc.query_selector("#subtree").unwrap().unwrap();
    let inline_id = doc.query_selector("#inline").unwrap().unwrap();

    let anon_ids: Vec<_> = doc
        .get_node(subtree_id)
        .unwrap()
        .anonymous_blocks
        .iter()
        .copied()
        .collect();
    assert!(!anon_ids.is_empty());
    let before_id = doc.get_node(subtree_id).unwrap().before().unwrap();
    let after_id = doc.get_node(subtree_id).unwrap().after().unwrap();

    doc.mutate().remove_node(subtree_id);

    for anon_id in anon_ids {
        assert!(doc.get_node(anon_id).is_none());
    }
    assert!(doc.get_node(before_id).is_none());
    assert!(doc.get_node(after_id).is_none());
    assert!(
        doc.get_node(subtree_id)
            .unwrap()
            .anonymous_blocks
            .is_empty()
    );
    assert_eq!(doc.get_node(subtree_id).unwrap().before(), None);
    assert_eq!(doc.get_node(subtree_id).unwrap().after(), None);

    // Reparent a former descendant back into the live document and resolve.
    doc.mutate().append_children(host_id, &[inline_id]);
    doc.resolve(0.0);

    assert_eq!(
        doc.get_node(inline_id).unwrap().layout_parent.get(),
        Some(host_id)
    );
}

/// Transitioning a subtree to `display: none` must tear down descendant
/// box-tree pointers, anonymous blocks, pseudo-elements, and OOF hoists, and
/// transitioning back to `display: block` must reconstruct cleanly.
#[test]
fn display_none_tears_down_descendant_box_tree_and_oof_hoists() {
    let mut doc = make_doc(
        r#"<!DOCTYPE html>
        <html><head><style>
            #inner::before { content: "hi"; display: block; }
        </style></head><body>
            <div id="cb" style="position: relative; width: 200px; height: 200px;">
                <div id="wrapper">
                    <div id="inner">
                        <div id="block">b</div>
                        <span id="inline">t</span>
                    </div>
                    <div id="oof" style="position: absolute; top: 5px; left: 5px; width: 20px; height: 20px;"></div>
                </div>
            </div>
        </body></html>"#,
    );

    let cb_id = doc.query_selector("#cb").unwrap().unwrap();
    let wrapper_id = doc.query_selector("#wrapper").unwrap().unwrap();
    let inner_id = doc.query_selector("#inner").unwrap().unwrap();
    let oof_id = doc.query_selector("#oof").unwrap().unwrap();

    let inner_before = doc.get_node(inner_id).unwrap().before().unwrap();
    let inner_anons: Vec<_> = doc
        .get_node(inner_id)
        .unwrap()
        .anonymous_blocks
        .iter()
        .copied()
        .collect();
    assert!(!inner_anons.is_empty());
    assert!(
        doc.get_node(cb_id)
            .unwrap()
            .hoisted_children
            .borrow()
            .contains(&oof_id)
    );

    doc.mutate()
        .set_style_property(wrapper_id, "display", "none");
    doc.resolve(0.0);

    assert!(doc.get_node(inner_before).is_none());
    for anon_id in inner_anons {
        assert!(doc.get_node(anon_id).is_none());
    }
    assert_eq!(doc.get_node(inner_id).unwrap().layout_parent.get(), None);
    assert_eq!(doc.get_node(oof_id).unwrap().layout_parent.get(), None);
    assert_eq!(
        doc.get_node(oof_id).unwrap().oof_containing_block.get(),
        None
    );
    assert!(
        !doc.get_node(cb_id)
            .unwrap()
            .hoisted_children
            .borrow()
            .contains(&oof_id)
    );

    // Restore visibility and ensure everything reconstructs cleanly.
    doc.mutate()
        .set_style_property(wrapper_id, "display", "block");
    doc.resolve(0.0);

    assert_eq!(
        doc.get_node(inner_id).unwrap().layout_parent.get(),
        Some(wrapper_id)
    );
    assert!(doc.get_node(inner_id).unwrap().before().is_some());
    assert_eq!(
        doc.get_node(oof_id).unwrap().oof_containing_block.get(),
        Some(cb_id)
    );
    assert!(
        doc.get_node(cb_id)
            .unwrap()
            .hoisted_children
            .borrow()
            .contains(&oof_id)
    );
}

/// An inline element inside an inline formatting context switching to
/// `display: none` must clear its descendant box-tree state and recover cleanly
/// when restored.
#[test]
fn display_none_inside_inline_root_tears_down_subtree() {
    let mut doc = make_doc(
        r#"<!DOCTYPE html>
        <html><head><style>
            #target::before { content: "x"; }
        </style></head><body>
            <p id="root">
                hello
                <span id="target">
                    <span id="badge" style="display: inline-block; width: 20px; height: 20px;"></span>
                </span>
            </p>
        </body></html>"#,
    );

    let target_id = doc.query_selector("#target").unwrap().unwrap();
    let badge_id = doc.query_selector("#badge").unwrap().unwrap();
    assert!(
        doc.get_node(badge_id)
            .unwrap()
            .layout_parent
            .get()
            .is_some()
    );

    doc.mutate()
        .set_style_property(target_id, "display", "none");
    doc.resolve(0.0);

    assert_eq!(doc.get_node(target_id).unwrap().layout_parent.get(), None);
    assert_eq!(doc.get_node(badge_id).unwrap().layout_parent.get(), None);
    assert_eq!(doc.get_node(target_id).unwrap().before(), None);

    doc.mutate()
        .set_style_property(target_id, "display", "inline");
    doc.resolve(0.0);

    assert!(
        doc.get_node(badge_id)
            .unwrap()
            .layout_parent
            .get()
            .is_some()
    );
}

/// Switching a container to `display: contents` must clear its own box-tree
/// state (anonymous blocks, `layout_children`, `layout_parent`) while hoisting
/// its children cleanly into the outer container.
#[test]
fn display_contents_clears_own_box_state_and_reconstructs_children() {
    let mut doc = make_doc(
        r#"<!DOCTYPE html>
        <html><body>
            <div id="outer">
                <div id="mid">
                    <div id="child" style="width: 40px; height: 40px;"></div>
                </div>
            </div>
        </body></html>"#,
    );

    let outer_id = doc.query_selector("#outer").unwrap().unwrap();
    let mid_id = doc.query_selector("#mid").unwrap().unwrap();
    let child_id = doc.query_selector("#child").unwrap().unwrap();
    assert_eq!(
        doc.get_node(child_id).unwrap().layout_parent.get(),
        Some(mid_id)
    );

    doc.mutate()
        .set_style_property(mid_id, "display", "contents");
    doc.resolve(0.0);

    assert_eq!(doc.get_node(mid_id).unwrap().layout_parent.get(), None);
    assert!(
        doc.get_node(mid_id)
            .unwrap()
            .layout_children
            .borrow()
            .is_none()
    );
    assert_eq!(
        doc.get_node(child_id).unwrap().layout_parent.get(),
        Some(outer_id)
    );

    doc.mutate().set_style_property(mid_id, "display", "block");
    doc.resolve(0.0);

    assert_eq!(
        doc.get_node(mid_id).unwrap().layout_parent.get(),
        Some(outer_id)
    );
    assert_eq!(
        doc.get_node(child_id).unwrap().layout_parent.get(),
        Some(mid_id)
    );
}

/// Moving an in-document out-of-flow node directly into an already
/// `display: none` parent must clear its box-tree pointers and unlink it from
/// its former containing block's `hoisted_children`.
#[test]
fn in_document_move_into_display_none_clears_oof_and_box_state() {
    let mut doc = make_doc(
        r#"<!DOCTYPE html>
        <html><body>
            <div id="cb" style="position: relative; width: 200px; height: 200px;">
                <div id="oof" style="position: absolute; top: 0; left: 0; width: 20px; height: 20px;"></div>
            </div>
            <div id="hidden" style="display: none;"></div>
        </body></html>"#,
    );

    let cb_id = doc.query_selector("#cb").unwrap().unwrap();
    let oof_id = doc.query_selector("#oof").unwrap().unwrap();
    let hidden_id = doc.query_selector("#hidden").unwrap().unwrap();

    assert_eq!(
        doc.get_node(oof_id).unwrap().oof_containing_block.get(),
        Some(cb_id)
    );

    doc.mutate().append_children(hidden_id, &[oof_id]);
    doc.resolve(0.0);

    assert_eq!(doc.get_node(oof_id).unwrap().layout_parent.get(), None);
    assert_eq!(
        doc.get_node(oof_id).unwrap().oof_containing_block.get(),
        None
    );
    assert!(
        !doc.get_node(cb_id)
            .unwrap()
            .hoisted_children
            .borrow()
            .contains(&oof_id)
    );
}
