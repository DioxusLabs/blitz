//! The paint tree (`paint_children` / `stacking_context`) is built once per
//! frame after layout, only for damaged subtrees, and holds topology only:
//! hoisted boxes' offsets from their stacking-context root are derived from
//! the current layout (and scroll offsets) at use time.

use blitz_dom::{DocumentConfig, MEMOISE_GEOMETRY, hoisted_child_position};
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
            incremental: Some(incremental),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn id(doc: &HtmlDocument, selector: &str) -> NodeId {
    doc.query_selector(selector)
        .unwrap()
        .unwrap_or_else(|| panic!("no node matches {selector}"))
}

/// Absolute (document) position of the node's border box, from layout.
fn abs_pos(doc: &HtmlDocument, node_id: NodeId) -> (f32, f32) {
    let node = doc.get_node(node_id).unwrap();
    let mut x = node.final_layout().location.x;
    let mut y = node.final_layout().location.y;
    let mut current = node.layout_parent.get();
    while let Some(parent_id) = current {
        let parent = doc.get_node(parent_id).unwrap();
        x += parent.final_layout().location.x - parent.scroll_offset().x as f32;
        y += parent.final_layout().location.y - parent.scroll_offset().y as f32;
        current = parent.layout_parent.get();
    }
    (x, y)
}

fn center(doc: &HtmlDocument, selector: &str) -> (f32, f32) {
    let node_id = id(doc, selector);
    let (x, y) = abs_pos(doc, node_id);
    let size = doc.get_node(node_id).unwrap().final_layout().size;
    (x + size.width / 2.0, y + size.height / 2.0)
}

fn hit(doc: &HtmlDocument, x: f32, y: f32) -> NodeId {
    doc.hit(x, y)
        .unwrap_or_else(|| panic!("nothing hit at ({x}, {y})"))
        .node_id
}

/// `(z_index, offset of the hoisted child's border box from the SC root)` for
/// `child` in `sc_root`'s stacking context.
fn hoisted_entry(doc: &HtmlDocument, sc_root: NodeId, child: NodeId) -> Option<(i32, (f32, f32))> {
    let root = doc.get_node(sc_root).unwrap();
    let sc = root.stacking_context.as_ref()?;
    let entry = sc.children.iter().find(|c| c.node_id == child)?;
    let offset = entry.position(doc.tree(), sc_root);
    let location = doc.get_node(child).unwrap().final_layout().location;
    Some((
        entry.z_index,
        (offset.x + location.x, offset.y + location.y),
    ))
}

fn hover(doc: &mut HtmlDocument, selector: &str) {
    let (x, y) = center(doc, selector);
    assert!(doc.set_hover_to(x, y), "hover {selector}");
    doc.resolve(0.0);
}

fn unhover(doc: &mut HtmlDocument) {
    assert!(doc.set_hover_to(399.0, 399.0));
    doc.resolve(0.0);
}

/// DOM depth of the node (number of ancestors).
fn depth(doc: &HtmlDocument, node_id: NodeId) -> usize {
    let mut depth = 0;
    let mut current = doc.get_node(node_id).unwrap().parent;
    while let Some(parent) = current {
        depth += 1;
        current = doc.get_node(parent).unwrap().parent;
    }
    depth
}

const STALE_POSITION: &str = r#"<html><head><style>
    body { margin: 0; }
    #a { height: 20px; background: green; }
    #a:hover { height: 60px; }
    #b { position: relative; z-index: 1; height: 20px; width: 100px; background: red; }
</style></head><body>
    <div id="a"></div>
    <div id="b"></div>
</body></html>"#;

/// A hover on A grows it, moving the z-indexed (hoisted) sibling B down. In
/// the same frame, hit-testing and the render-side offset must see B at its
/// new location (the paint tree used to be built before layout, so hoisted
/// offsets lagged a frame behind).
#[test]
fn hoisted_position_follows_layout_in_same_frame() {
    for incremental in [true, false] {
        let mut doc = make_doc(STALE_POSITION, incremental);
        let a = id(&doc, "#a");
        let b = id(&doc, "#b");
        let root = doc.root_element().id;

        assert_eq!(hit(&doc, 50.0, 30.0), b);
        assert_eq!(hoisted_entry(&doc, root, b), Some((1, (0.0, 20.0))));

        // Hover A (single resolve): A is 60px tall, B starts at y=60.
        assert!(doc.set_hover_to(50.0, 10.0));
        doc.resolve(0.0);
        assert_eq!(doc.get_node(a).unwrap().final_layout().size.height, 60.0);
        assert_eq!(abs_pos(&doc, b), (0.0, 60.0));

        assert_eq!(hit(&doc, 50.0, 70.0), b, "incremental={incremental}");
        assert_ne!(hit(&doc, 50.0, 30.0), b, "incremental={incremental}");
        assert_eq!(
            hoisted_entry(&doc, root, b),
            Some((1, (0.0, 60.0))),
            "incremental={incremental}"
        );
        assert_eq!(
            hoisted_child_position(doc.tree(), root, b),
            taffy::Point { x: 0.0, y: 0.0 }
        );
    }
}

const REPLAY: &str = r#"<html><head><style>
    body { margin: 0; }
    #r { opacity: 0.5; }
    #s1, #s2 { padding: 5px; }
    #z { position: relative; z-index: 1; height: 10px; width: 10px; background: red; }
    #z:hover { z-index: 3; }
    #w { height: 10px; width: 100px; background: blue; }
    #w:hover { width: 50px; }
    #c { height: 10px; width: 100px; background: green; }
    #c:hover { background: yellow; }
</style></head><body>
    <div id="r">
        <div id="s1"><div id="s1-inner"><div id="z"></div></div></div>
        <div id="s2"><div id="w"></div><div id="c"></div></div>
    </div>
</body></html>"#;

/// The stacking-context root R is rebuilt when layout damage inside S2 reaches
/// it, but S1 is clean and skipped: its hoisted entry in R must be replayed.
/// Changing `z-index` inside S1 damages S1, so the entry is rebuilt.
#[test]
fn clean_subtree_entries_are_replayed() {
    for incremental in [true, false] {
        let mut doc = make_doc(REPLAY, incremental);
        let r = id(&doc, "#r");
        let z = id(&doc, "#z");
        let s1 = id(&doc, "#s1");

        assert!(doc.get_node(r).unwrap().stacking_context.is_some());
        assert_eq!(hoisted_entry(&doc, r, z), Some((1, (5.0, 5.0))));
        let s1_cache_len = doc
            .get_node(s1)
            .unwrap()
            .sc_contribution_cache
            .borrow()
            .len();
        assert_eq!(s1_cache_len, 1, "S1 caches the entry it contributed to R");

        hover(&mut doc, "#w");
        assert_eq!(
            doc.get_node(id(&doc, "#w"))
                .unwrap()
                .final_layout()
                .size
                .width,
            50.0
        );
        assert_eq!(
            hoisted_entry(&doc, r, z),
            Some((1, (5.0, 5.0))),
            "incremental={incremental}: S1's entry is still present in R"
        );
        assert_eq!(
            doc.get_node(r)
                .unwrap()
                .stacking_context
                .as_ref()
                .unwrap()
                .children
                .len(),
            1
        );

        unhover(&mut doc);
        hover(&mut doc, "#c");
        assert_eq!(hoisted_entry(&doc, r, z), Some((1, (5.0, 5.0))));

        unhover(&mut doc);
        hover(&mut doc, "#z");
        assert_eq!(
            hoisted_entry(&doc, r, z),
            Some((3, (5.0, 5.0))),
            "incremental={incremental}: z-index change rebuilds the entry"
        );
        unhover(&mut doc);
        assert_eq!(hoisted_entry(&doc, r, z), Some((1, (5.0, 5.0))));
    }
}

/// In incremental mode a hover visits only the damaged chain of ancestors
/// (O(depth)), not the whole tree.
#[cfg(debug_assertions)]
#[test]
fn hover_visits_only_damaged_ancestors() {
    let mut doc = make_doc(REPLAY, true);
    let total = doc.tree().len();
    let c = id(&doc, "#c");

    hover(&mut doc, "#c");
    let visits = doc.paint_tree_visits();
    assert!(
        visits <= depth(&doc, c) + 1,
        "colour-only hover visited {visits} nodes (depth {}, tree {total})",
        depth(&doc, c)
    );

    unhover(&mut doc);
    hover(&mut doc, "#w");
    let visits = doc.paint_tree_visits();
    assert!(
        visits <= depth(&doc, c) + 1,
        "width hover visited {visits} nodes"
    );

    // Non-incremental mode visits every node with a display style.
    let mut doc = make_doc(REPLAY, false);
    hover(&mut doc, "#c");
    assert!(doc.paint_tree_visits() > depth(&doc, c) + 1);
}

const PERCENT: &str = r#"<html><head><style>
    body { margin: 0; }
    #b { position: relative; z-index: 1; margin-left: 50%; width: 20px; height: 20px; background: red; }
</style></head><body>
    <div id="b"></div>
</body></html>"#;

/// A viewport resize relays out from the root without any damage bits when
/// only percentages are involved, so no paint list is rebuilt. Hoisted
/// offsets are derived from the current layout, so they follow anyway.
#[test]
fn viewport_resize_moves_hoisted_box_without_rebuild() {
    for incremental in [true, false] {
        let mut doc = make_doc(PERCENT, incremental);
        let b = id(&doc, "#b");
        let root = doc.root_element().id;
        assert_eq!(abs_pos(&doc, b), (200.0, 0.0));
        assert_eq!(hit(&doc, 210.0, 10.0), b);

        doc.set_viewport(Viewport::new(200, 400, 1.0, ColorScheme::Light));
        doc.resolve(0.0);
        assert_eq!(abs_pos(&doc, b), (100.0, 0.0));
        assert_eq!(hit(&doc, 110.0, 10.0), b, "incremental={incremental}");
        assert_ne!(hit(&doc, 90.0, 10.0), b, "incremental={incremental}");
        assert_eq!(hoisted_entry(&doc, root, b), Some((1, (100.0, 0.0))));
    }
}

const SCROLL: &str = r#"<html><head><style>
    body { margin: 0; }
    #s { position: relative; overflow: auto; height: 50px; width: 200px; }
    #spacer { height: 100px; }
    #b { position: relative; z-index: 1; height: 20px; width: 100px; background: red; }
    #tail { height: 200px; }
</style></head><body>
    <div id="s"><div id="spacer"></div><div id="b"></div><div id="tail"></div></div>
</body></html>"#;

/// Scrolling changes `scroll_offset` without a `resolve()`; hit-testing must
/// see the scrolled position of hoisted children immediately.
#[test]
fn scroll_moves_hoisted_box_without_resolve() {
    for incremental in [true, false] {
        let mut doc = make_doc(SCROLL, incremental);
        let s = id(&doc, "#s");
        let b = id(&doc, "#b");
        let root = doc.root_element().id;

        // B is at y=100 inside a 50px scroller: not visible.
        assert_ne!(hit(&doc, 50.0, 20.0), b);
        assert_eq!(hoisted_entry(&doc, root, b), Some((1, (0.0, 100.0))));

        let generation = doc.tree().geometry_generation();
        doc.scroll_node_by(s, 0.0, -90.0, |_| {});
        assert_eq!(doc.get_node(s).unwrap().scroll_offset().y, 90.0);
        assert_ne!(doc.tree().geometry_generation(), generation);

        // No resolve: B now spans y=10..30 in the viewport.
        assert_eq!(hit(&doc, 50.0, 20.0), b, "incremental={incremental}");
        assert_eq!(hoisted_entry(&doc, root, b), Some((1, (0.0, 10.0))));

        doc.resolve(0.0);
        assert_eq!(hit(&doc, 50.0, 20.0), b);
    }
}

/// Hoisted offsets are computed at most once per geometry generation.
#[test]
fn hoisted_position_is_memoised_per_generation() {
    if !MEMOISE_GEOMETRY {
        return;
    }
    let mut doc = make_doc(SCROLL, true);
    let s = id(&doc, "#s");
    let b = id(&doc, "#b");
    let root = doc.root_element().id;

    let entry = |doc: &HtmlDocument| {
        doc.get_node(root)
            .unwrap()
            .stacking_context
            .as_ref()
            .unwrap()
            .children
            .iter()
            .find(|c| c.node_id == b)
            .unwrap()
            .position_is_cached(doc.tree())
    };

    // Freshly built: nothing computed yet.
    assert!(!entry(&doc));
    let _ = doc.hit(50.0, 20.0);
    assert!(entry(&doc), "first use computes and caches the offset");
    let generation = doc.tree().geometry_generation();
    let _ = doc.hit(50.0, 20.0);
    assert_eq!(doc.tree().geometry_generation(), generation);
    assert!(entry(&doc));

    // Layout ran: the cache is stale until next use.
    doc.resolve(0.0);
    assert!(!entry(&doc));
    let _ = doc.hit(50.0, 20.0);
    assert!(entry(&doc));

    // Scrolled: likewise.
    doc.scroll_node_by(s, 0.0, -90.0, |_| {});
    assert!(!entry(&doc));
    assert_eq!(hit(&doc, 50.0, 20.0), b);
    assert!(entry(&doc));
}

const FLEX_ITEM_ROLE: &str = r#"<html><head><style>
    body { margin: 0; }
    #c { display: flex; width: 100px; height: 100px; }
    #c:hover { display: block; }
    #item { z-index: 3; width: 100px; height: 100px; }
    #inner { position: relative; z-index: 3; width: 100px; height: 100px; background: red; }
    #ext { position: absolute; left: 0; top: 0; z-index: 2; width: 100px; height: 100px; background: blue; }
</style></head><body>
    <div id="c"><div id="item"><div id="inner"></div></div></div>
    <div id="ext"></div>
</body></html>"#;

/// Whether a static z-indexed child is a stacking-context root depends on
/// its parent being a flex/grid container. Toggling the parent's `display`
/// damages only the parent, so the child's role must be re-derived even
/// though its own damage says it is clean.
#[test]
fn flex_item_role_change_rebuilds_child() {
    for incremental in [false, true] {
        let mut doc = make_doc(FLEX_ITEM_ROLE, incremental);
        let item = id(&doc, "#item");
        let inner = id(&doc, "#inner");

        // Flex: `#item` is an SC root (z-index 3 flex item) above `#ext` (2).
        assert!(doc.get_node(item).unwrap().stacking_context.is_some());
        assert!(hoisted_entry(&doc, item, inner).is_some());
        assert_eq!(hit(&doc, 50.0, 50.0), inner);

        // Block: `#item` is a plain static block; `#inner` (z-index 3) is
        // hoisted to the root stacking context, still above `#ext`.
        hover(&mut doc, "#inner");
        assert!(
            doc.get_node(item).unwrap().stacking_context.is_none(),
            "incremental={incremental}"
        );
        assert!(hoisted_entry(&doc, item, inner).is_none());
        assert_eq!(hit(&doc, 50.0, 50.0), inner, "incremental={incremental}");

        // Back to flex: `#item` becomes an SC root again and `#inner` must
        // leave the root stacking context (otherwise it would paint twice /
        // at the wrong level) and be owned by `#item`.
        unhover(&mut doc);
        assert!(
            doc.get_node(item).unwrap().stacking_context.is_some(),
            "incremental={incremental}"
        );
        assert!(hoisted_entry(&doc, item, inner).is_some());
        let html = doc.root_element().id;
        assert!(hoisted_entry(&doc, html, inner).is_none());
        assert_eq!(hit(&doc, 50.0, 50.0), inner, "incremental={incremental}");
    }
}

const REPARENT_ROLE: &str = r#"<html><head><style>
    body { margin: 0; }
    #flex { display: flex; width: 100px; height: 100px; }
    #block { position: absolute; left: 0; top: 0; width: 100px; height: 100px; }
    #item { z-index: 3; width: 100px; height: 100px; }
    #inner { position: relative; z-index: 3; width: 100px; height: 100px; background: red; }
    #ext { position: absolute; left: 0; top: 0; z-index: 2; width: 100px; height: 100px; background: blue; }
</style></head><body>
    <div id="flex"><div id="item"><div id="inner"></div></div></div>
    <div id="block"></div>
    <div id="ext"></div>
</body></html>"#;

/// Moving a clean subtree from a flex container into a block container
/// changes the moved node's stacking-context role without a style change on
/// the node itself.
#[test]
fn reparent_from_flex_to_block_rebuilds_child() {
    for incremental in [false, true] {
        let mut doc = make_doc(REPARENT_ROLE, incremental);
        let item = id(&doc, "#item");
        let inner = id(&doc, "#inner");
        let block = id(&doc, "#block");
        assert!(doc.get_node(item).unwrap().stacking_context.is_some());
        assert_eq!(hit(&doc, 50.0, 50.0), inner);

        doc.mutate().append_children(block, &[item]);
        doc.resolve(0.0);

        assert!(
            doc.get_node(item).unwrap().stacking_context.is_none(),
            "incremental={incremental}"
        );
        assert_eq!(hit(&doc, 50.0, 50.0), inner, "incremental={incremental}");
    }
}
