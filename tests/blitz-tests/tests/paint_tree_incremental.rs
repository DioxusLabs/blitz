//! The paint tree (`paint_children` / `stacking_context`) is built once per
//! frame after layout, only for damaged subtrees, and holds topology only:
//! hoisted boxes' offsets from their stacking-context root are derived from
//! the current layout (and scroll offsets) at use time.

use blitz_dom::{DocumentConfig, hoisted_child_position};
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
    let mut current = node.containing_block();
    while let Some(parent_id) = current {
        let parent = doc.get_node(parent_id).unwrap();
        x += parent.final_layout().location.x - parent.scroll_offset().x as f32;
        y += parent.final_layout().location.y - parent.scroll_offset().y as f32;
        current = parent.containing_block();
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

const HOISTED_OVERFLOW: &str = r#"<html><head><style>
    body { margin: 0; }
    #r { position: relative; z-index: 0; width: 100px; height: 100px; }
    #b { position: relative; z-index: 1; width: 10px; height: 10px; }
    #c { position: absolute; left: 150px; top: 150px; width: 20px; height: 20px; }
    #t { position: relative; z-index: 1; width: 10px; height: 10px; transform: translate(150px, 250px); }
</style></head><body>
    <div id="r">
        <div id="b"><div id="c"></div></div>
        <div id="t"></div>
    </div>
</body></html>"#;

/// A hoisted entry's descendants (and its transform) can extend past its
/// border box; the stacking-context root's hoisted bounding box must cover
/// them or the entry is never hit-tested there.
#[test]
fn hoisted_bbox_covers_overflow_and_transform() {
    let doc = make_doc(HOISTED_OVERFLOW, true);
    assert_eq!(hit(&doc, 160.0, 160.0), id(&doc, "#c"));
    assert_eq!(hit(&doc, 155.0, 265.0), id(&doc, "#t"));
}

const SCROLLED_SC_ROOT: &str = r#"<html><head><style>
    body { margin: 0; }
    #s { overflow: auto; height: 50px; width: 200px; transform: translateX(0px); }
    #spacer { height: 100px; }
    #b { position: relative; z-index: 1; height: 20px; width: 100px; }
    #tail { height: 200px; }
</style></head><body>
    <div id="s"><div id="spacer"></div><div id="b"></div><div id="tail"></div></div>
</body></html>"#;

/// The hoisted-content bbox is in the stacking-context root's unscrolled
/// content coordinates, so it must be compared against the point *after* the
/// root's scroll offset is applied: hoisted children of a scrolled root must
/// stay hit-testable.
#[test]
fn hoisted_children_of_scrolled_sc_root_are_hit() {
    for incremental in [true, false] {
        let mut doc = make_doc(SCROLLED_SC_ROOT, incremental);
        let s = id(&doc, "#s");
        let b = id(&doc, "#b");
        assert_eq!(hoisted_entry(&doc, s, b), Some((1, (0.0, 100.0))));
        assert_ne!(hit(&doc, 50.0, 20.0), b);

        doc.scroll_node_by(s, 0.0, -90.0, |_| {});
        assert_eq!(doc.get_node(s).unwrap().scroll_offset().y, 90.0);
        // B now spans y=10..30 within the 50px scroller.
        assert_eq!(hit(&doc, 50.0, 20.0), b, "incremental={incremental}");

        doc.resolve(0.0);
        assert_eq!(hit(&doc, 50.0, 20.0), b, "incremental={incremental}");
    }
}
||||||| parent of 5ea0fe76 (Integrate Taffy out-of-flow hoisting into the single post-layout paint-tree pass)


fn paint_children(doc: &HtmlDocument, node_id: NodeId) -> Vec<NodeId> {
    doc.get_node(node_id)
        .unwrap()
        .paint_children
        .borrow()
        .iter()
        .flatten()
        .copied()
        .collect()
}

const OOF_OWNERSHIP: &str = r#"<html><head><style>
    body { margin: 0; }
    #cb { position: relative; margin: 50px; width: 300px; height: 300px; }
    #anc { padding: 20px; }
    #anc:hover { transform: translateX(0px); }
    #spacer { height: 30px; }
    #spacer:hover { height: 90px; }
    #fixed { position: fixed; top: 10px; left: 10px; width: 20px; height: 20px; }
    #abs { position: absolute; width: 20px; height: 20px; }
    #sib { position: relative; height: 10px; }
</style></head><body>
    <div id="cb"><div id="anc"><div id="spacer"></div><div id="abs"></div><div id="fixed"></div></div><div id="sib"></div></div>
</body></html>"#;

/// An out-of-flow box is painted (and hit-tested) by its containing block,
/// not its DOM parent; when a hover makes the parent the containing block the
/// box moves between the two paint lists in the same frame.
#[test]
fn oof_box_is_owned_by_containing_block() {
    for incremental in [true, false] {
        let mut doc = make_doc(OOF_OWNERSHIP, incremental);
        let cb = id(&doc, "#cb");
        let anc = id(&doc, "#anc");
        let abs = id(&doc, "#abs");
        let fixed = id(&doc, "#fixed");
        let sib = id(&doc, "#sib");
        let root = doc.get_node(fixed).unwrap().containing_block().unwrap();
        assert_ne!(root, anc);

        let msg = format!("incremental={incremental}");
        assert!(!paint_children(&doc, anc).contains(&abs), "{msg}");
        assert!(!paint_children(&doc, anc).contains(&fixed), "{msg}");
        let cb_paint = paint_children(&doc, cb);
        assert!(cb_paint.contains(&abs), "{msg}: abs owned by #cb");
        assert!(
            cb_paint.iter().position(|&n| n == abs) < cb_paint.iter().position(|&n| n == sib),
            "{msg}: tree order among positioned boxes: {cb_paint:?}"
        );
        assert!(
            paint_children(&doc, root).contains(&fixed),
            "{msg}: fixed owned by root"
        );
        assert_eq!(abs_pos(&doc, fixed), (10.0, 10.0), "{msg}");
        assert_eq!(hit(&doc, 20.0, 20.0), fixed, "{msg}");
        let (ax, ay) = center(&doc, "#abs");
        assert_eq!(hit(&doc, ax, ay), abs, "{msg}");

        hover(&mut doc, "#anc");
        assert_eq!(
            doc.get_node(fixed).unwrap().containing_block(),
            Some(anc),
            "{msg}"
        );
        assert!(
            paint_children(&doc, anc).contains(&fixed),
            "{msg}: fixed moved to #anc"
        );
        assert!(
            !paint_children(&doc, root).contains(&fixed),
            "{msg}: removed from root"
        );
        assert!(
            paint_children(&doc, anc).contains(&abs),
            "{msg}: abs moved to #anc"
        );
        assert!(
            !paint_children(&doc, cb).contains(&abs),
            "{msg}: removed from #cb"
        );
        assert_eq!(abs_pos(&doc, fixed), (60.0, 60.0), "{msg}");
        assert_eq!(hit(&doc, 70.0, 70.0), fixed, "{msg}");

        unhover(&mut doc);
        assert!(!paint_children(&doc, anc).contains(&fixed), "{msg}");
        assert!(paint_children(&doc, root).contains(&fixed), "{msg}");
        assert!(paint_children(&doc, cb).contains(&abs), "{msg}");
        assert_eq!(hit(&doc, 20.0, 20.0), fixed, "{msg}");
    }
}

const OOF_Z_INDEX: &str = r#"<html><head><style>
    body { margin: 0; }
    #cb { position: relative; z-index: 0; margin: 50px; width: 300px; height: 300px; }
    #spacer { height: 30px; }
    #spacer:hover { height: 90px; }
    #abs { position: absolute; z-index: 1; width: 20px; height: 20px; }
</style></head><body>
    <div id="cb"><div id="anc"><div id="spacer"></div><div id="abs"></div></div></div>
</body></html>"#;

/// A z-indexed out-of-flow box at its static position is hoisted to the
/// stacking context enclosing its containing block, and its derived offset
/// follows the static position when in-flow content above it grows.
#[test]
fn z_indexed_oof_box_position_follows_static_position() {
    for incremental in [true, false] {
        let mut doc = make_doc(OOF_Z_INDEX, incremental);
        let cb = id(&doc, "#cb");
        let abs = id(&doc, "#abs");
        let msg = format!("incremental={incremental}");

        assert!(
            doc.get_node(cb).unwrap().stacking_context.is_some(),
            "{msg}"
        );
        assert_eq!(
            hoisted_entry(&doc, cb, abs),
            Some((1, (0.0, 30.0))),
            "{msg}"
        );
        assert_eq!(abs_pos(&doc, abs), (50.0, 80.0), "{msg}");
        assert_eq!(hit(&doc, 60.0, 90.0), abs, "{msg}");

        hover(&mut doc, "#spacer");
        assert_eq!(abs_pos(&doc, abs), (50.0, 140.0), "{msg}");
        assert_eq!(
            hoisted_entry(&doc, cb, abs),
            Some((1, (0.0, 90.0))),
            "{msg}"
        );
        assert_eq!(hit(&doc, 60.0, 150.0), abs, "{msg}");
    }
}

const OOF_EFFECT: &str = r#"<html><head><style>
    body { margin: 0; }
    #cb { position: relative; margin: 50px; padding-top: 20px; width: 300px; height: 280px; }
    #sp { height: 10px; }
    #sp:hover { height: 30px; }
    #fx { opacity: 0.5; margin: 0 20px; height: 100px; }
    #c { height: 10px; background: red; }
    #c:hover { background: blue; }
    #abs { position: absolute; top: 100px; left: 100px; width: 20px; height: 20px; }
</style></head><body>
    <div id="cb"><div id="sp"></div><div id="fx"><div id="c"></div><div id="abs"></div></div></div>
</body></html>"#;

/// An out-of-flow box below an atomic paint-effect ancestor that is not its
/// containing block is painted inside that ancestor's stacking context with a
/// compensated offset (derived per frame), and exactly once across clean
/// frames, frames rebuilding only the containing block, and full rebuilds.
#[test]
fn oof_box_under_effect_ancestor_paints_inside_it() {
    for incremental in [true, false] {
        let mut doc = make_doc(OOF_EFFECT, incremental);
        let cb = id(&doc, "#cb");
        let fx = id(&doc, "#fx");
        let abs = id(&doc, "#abs");
        let msg = format!("incremental={incremental}");

        let check = |doc: &HtmlDocument, fx_y: f32| {
            assert_eq!(
                doc.get_node(fx).unwrap().final_layout().location.y,
                fx_y,
                "{msg}"
            );
            assert!(!paint_children(doc, cb).contains(&abs), "{msg}");
            assert!(!paint_children(doc, fx).contains(&abs), "{msg}");
            let sc = doc.get_node(fx).unwrap().stacking_context.as_ref().unwrap();
            assert_eq!(
                sc.children.iter().filter(|c| c.node_id == abs).count(),
                1,
                "{msg}: exactly one entry for #abs in #fx"
            );
            // #abs is at (100, 100) in #cb; #fx is at (20, fx_y) in #cb.
            assert_eq!(
                hoisted_entry(doc, fx, abs),
                Some((0, (80.0, 100.0 - fx_y))),
                "{msg}"
            );
            assert_eq!(abs_pos(doc, abs), (150.0, 150.0), "{msg}");
            assert_eq!(hit(doc, 160.0, 160.0), abs, "{msg}");
        };
        check(&doc, 30.0);

        // Colour-only change: nothing is rebuilt.
        hover(&mut doc, "#c");
        check(&doc, 30.0);
        unhover(&mut doc);

        // #sp grows: #cb (the containing block) is rebuilt, #fx is clean.
        hover(&mut doc, "#sp");
        check(&doc, 50.0);
        unhover(&mut doc);
        check(&doc, 30.0);
    }
}

const FIXED_SCROLL: &str = r#"<html><head><style>
    body { margin: 0; height: 2000px; }
    #s { overflow: scroll; width: 200px; height: 100px; transform: translateX(0px); }
    #tall { height: 1000px; }
    #f0 { position: fixed; left: 10px; top: 10px; width: 20px; height: 20px; }
    #f1 { position: fixed; left: 50px; top: 10px; width: 20px; height: 20px; z-index: 1; }
    #g0 { position: fixed; left: 10px; top: 50px; width: 20px; height: 20px; }
    #g1 { position: fixed; left: 50px; top: 50px; width: 20px; height: 20px; z-index: 1; }
</style></head><body>
    <div id="s"><div id="tall"></div><div id="f0"></div><div id="f1"></div></div>
    <div id="g0"></div><div id="g1"></div>
</body></html>"#;

/// Fixed boxes stay put when their containing block (the viewport for `#g*`,
/// the transformed scroller `#s` for `#f*`) scrolls, whether they are placed
/// in `paint_children` (z-auto) or hoisted into the stacking context (z≠0).
#[test]
fn fixed_boxes_do_not_scroll_with_containing_block() {
    for incremental in [false, true] {
        let mut doc = make_doc(FIXED_SCROLL, incremental);
        let s = id(&doc, "#s");
        let (f0, f1, g0, g1) = (
            id(&doc, "#f0"),
            id(&doc, "#f1"),
            id(&doc, "#g0"),
            id(&doc, "#g1"),
        );
        assert_eq!(hit(&doc, 20.0, 20.0), f0);
        assert_eq!(hit(&doc, 60.0, 20.0), f1);
        assert_eq!(hit(&doc, 20.0, 60.0), g0);
        assert_eq!(hit(&doc, 60.0, 60.0), g1);

        doc.scroll_node_by(s, 0.0, -40.0, |_| {});
        assert_eq!(doc.get_node(s).unwrap().scroll_offset().y, 40.0);
        assert_eq!(
            hit(&doc, 20.0, 20.0),
            f0,
            "z-auto fixed in scrolled CB ({incremental})"
        );
        assert_eq!(
            hit(&doc, 60.0, 20.0),
            f1,
            "z-index fixed in scrolled CB ({incremental})"
        );

        doc.set_viewport_scroll(blitz_dom::Point { x: 0.0, y: 30.0 });
        assert_eq!(
            hit(&doc, 20.0, 60.0),
            g0,
            "z-auto fixed under viewport scroll ({incremental})"
        );
        assert_eq!(
            hit(&doc, 60.0, 60.0),
            g1,
            "z-index fixed under viewport scroll ({incremental})"
        );
    }
}
