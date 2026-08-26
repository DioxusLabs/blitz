//! A positioned non-atomic inline (e.g. `<a style="position: relative">`) is
//! the containing block for its `position: absolute` descendants even though it
//! has no layout box of its own (it is a style span of the inline root's text
//! layout): the descendants must be positioned against its fragment box rather
//! than bubbling up to an outer positioned block or the initial containing
//! block.
//!
//! The "stretched link" pattern (`a::after { position: absolute; inset: 0 }`)
//! relies on this; if the pseudo-element escaped to the ICB it would become an
//! invisible viewport-sized overlay that swallows hover and clicks for the
//! whole page.

use blitz_test_harness::{Harness, Rect};
use blitz_traits::node_id::NodeId;

fn after_pseudo(harness: &Harness, selector: &str) -> NodeId {
    let node = harness.node(selector);
    harness
        .base()
        .get_node(node)
        .unwrap()
        .after()
        .expect("element should have an ::after pseudo-element")
}

/// The per-line fragment rects of a non-atomic inline element, in page coordinates
fn fragment_rects(harness: &Harness, selector: &str) -> Vec<Rect> {
    let node = harness.node(selector);
    harness
        .base()
        .inline_fragment_rects(node)
        .expect("element should be a non-atomic inline")
        .into_iter()
        .map(|rect| Rect {
            x: rect.x as f32,
            y: rect.y as f32,
            width: rect.width as f32,
            height: rect.height as f32,
        })
        .collect()
}

/// The center of the first fragment of a non-atomic inline element
fn fragment_center(harness: &Harness, selector: &str) -> (f32, f32) {
    fragment_rects(harness, selector)[0].center()
}

fn assert_rect_eq(actual: Rect, expected: Rect, context: &str) {
    let close = |a: f32, b: f32| (a - b).abs() < 1.0;
    assert!(
        close(actual.x, expected.x)
            && close(actual.y, expected.y)
            && close(actual.width, expected.width)
            && close(actual.height, expected.height),
        "{context}: expected {expected:?}, got {actual:?}"
    );
}

/// Whether `node` is `ancestor` or a DOM descendant of it
fn is_within(harness: &Harness, mut node: NodeId, ancestor: NodeId) -> bool {
    let doc = harness.base();
    loop {
        if node == ancestor {
            return true;
        }
        match doc.get_node(node).and_then(|n| n.parent) {
            Some(parent) => node = parent,
            None => return false,
        }
    }
}

const STRETCHED_LINK_PAGE: &str = r#"<!DOCTYPE html>
<html><head><style>
    body { margin: 0; font-size: 16px; line-height: 20px; }
    #header { height: 40px; padding: 10px; }
    #spacer { height: 300px; }
    h2 { margin: 0 20px; width: 300px; }
    .stretched { position: relative; }
    .stretched::after { content: ""; position: absolute; inset: 0; z-index: 1; }
</style></head>
<body>
<div id="header"><a id="home" href="/">Home</a> <a id="news" href="/news">News</a></div>
<div id="spacer"></div>
<h2><a id="sport" class="stretched" href="/sport"><span id="label">Sport</span></a></h2>
</body></html>
"#;

#[test]
fn stretched_link_pseudo_is_bounded_to_the_inline() {
    let harness = Harness::from_html(STRETCHED_LINK_PAGE);
    let pseudo = after_pseudo(&harness, "#sport");

    let fragments = fragment_rects(&harness, "#sport");
    assert_eq!(fragments.len(), 1, "link should occupy a single line");
    assert_rect_eq(
        harness.layout_rect_of(pseudo),
        fragments[0],
        "::after with inset: 0 should cover the positioned inline's fragment box",
    );
}

#[test]
fn stretched_link_pseudo_does_not_swallow_hit_tests() {
    let harness = Harness::from_html(STRETCHED_LINK_PAGE);
    let pseudo = after_pseudo(&harness, "#sport");
    let home = harness.node("#home");
    let news = harness.node("#news");
    let sport = harness.node("#sport");

    let (x, y) = fragment_center(&harness, "#home");
    let hit = harness.hit_node(x, y);
    assert!(
        is_within(&harness, hit, home),
        "hit test over #home landed on {hit:?} (stretched pseudo is {pseudo:?})"
    );

    let (x, y) = fragment_center(&harness, "#news");
    let hit = harness.hit_node(x, y);
    assert!(
        is_within(&harness, hit, news),
        "hit test over #news landed on {hit:?}"
    );

    // The pseudo-element itself is hit over the link
    let (x, y) = harness.layout_rect_of(pseudo).center();
    assert_eq!(harness.hit_node(x, y), pseudo);

    // Empty page area hits neither
    let hit = harness.hit(700.0, 200.0).map(|hit| hit.node_id);
    assert!(
        hit.is_none_or(|hit| hit != pseudo && !is_within(&harness, hit, sport)),
        "hit test over empty page area landed on the stretched link: {hit:?}"
    );
}

#[test]
fn hover_moves_between_links_with_stretched_pseudo() {
    let mut harness = Harness::from_html(STRETCHED_LINK_PAGE);
    let home = harness.node("#home");
    let news = harness.node("#news");
    let sport = harness.node("#sport");

    let home_center = fragment_center(&harness, "#home");
    let news_center = fragment_center(&harness, "#news");
    let sport_center = fragment_center(&harness, "#label");

    for _ in 0..2 {
        harness.move_mouse_to(home_center.0, home_center.1);
        let hovered = harness.hovered().expect("hovering #home");
        assert!(is_within(&harness, hovered, home), "hovered {hovered:?}");

        harness.move_mouse_to(news_center.0, news_center.1);
        let hovered = harness.hovered().expect("hovering #news");
        assert!(is_within(&harness, hovered, news), "hovered {hovered:?}");

        harness.move_mouse_to(sport_center.0, sport_center.1);
        assert_eq!(harness.hovered(), Some(sport));

        harness.move_mouse_to(700.0, 200.0);
        let hovered = harness.hovered();
        assert!(
            hovered.is_none_or(|hovered| !is_within(&harness, hovered, sport)),
            "hovered {hovered:?} over empty page area"
        );
    }
}

#[test]
fn abspos_insets_resolve_against_positioned_inline() {
    let harness = Harness::from_html(
        r#"<!DOCTYPE html>
        <html><head><style>
            body { margin: 0; font-size: 16px; line-height: 20px; }
            #para { margin: 30px; width: 300px; }
            #rel { position: relative; }
            .abs { position: absolute; width: 20px; height: 10px; }
        </style></head>
        <body><p id="para">aaaa <span id="rel">bbbb bbbb<i id="tl" class="abs" style="left: 10px; top: 5px"></i><i id="br" class="abs" style="right: 0; bottom: 0"></i></span> aaaa</p></body></html>
        "#,
    );
    let fragments = fragment_rects(&harness, "#rel");
    assert_eq!(fragments.len(), 1);
    let cb = fragments[0];

    let tl = harness.layout_rect("#tl");
    assert!(
        (tl.x - (cb.x + 10.0)).abs() < 1.0 && (tl.y - (cb.y + 5.0)).abs() < 1.0,
        "expected top-left at ({}, {}), got {tl:?}",
        cb.x + 10.0,
        cb.y + 5.0
    );

    let br = harness.layout_rect("#br");
    assert!(
        ((br.x + br.width) - (cb.x + cb.width)).abs() < 1.0
            && ((br.y + br.height) - (cb.y + cb.height)).abs() < 1.0,
        "expected bottom-right at ({}, {}), got {br:?}",
        cb.x + cb.width,
        cb.y + cb.height
    );
}

#[test]
fn fragmented_inline_containing_block_spans_first_to_last_fragment() {
    let harness = Harness::from_html(
        r#"<!DOCTYPE html>
        <html><head><style>
            body { margin: 0; font-size: 16px; line-height: 20px; }
            #para { margin: 30px; width: 100px; }
            #rel { position: relative; }
            #abs { position: absolute; inset: 0; }
        </style></head>
        <body><p id="para">aa <span id="rel">bbbb bbbb bbbb bbbb bbbb bbbb<i id="abs"></i></span></p></body></html>
        "#,
    );
    let fragments = fragment_rects(&harness, "#rel");
    assert!(fragments.len() >= 2, "span should wrap: {fragments:?}");
    let first = fragments[0];
    let last = fragments[fragments.len() - 1];

    let abs = harness.layout_rect("#abs");
    assert_rect_eq(
        abs,
        Rect {
            x: first.x,
            y: first.y,
            width: last.x + last.width - first.x,
            height: last.y + last.height - first.y,
        },
        "inset: 0 box should span from the first fragment's top-left to the last's bottom-right",
    );
}

#[test]
fn abspos_escapes_to_positioned_block_when_no_inline_ancestor_is_positioned() {
    let harness = Harness::from_html(
        r#"<!DOCTYPE html>
        <html><head><style>
            body { margin: 0; font-size: 16px; line-height: 20px; }
            #outer { position: relative; margin: 50px; border: 3px solid; width: 300px; }
            #abs { position: absolute; left: 0; top: 0; width: 20px; height: 10px; }
        </style></head>
        <body><div id="outer"><p>aaaa <span id="plain">bbbb<i id="abs"></i></span></p></div></body></html>
        "#,
    );
    let outer = harness.layout_rect("#outer");
    let abs = harness.layout_rect("#abs");
    assert_eq!((abs.x, abs.y), (outer.x + 3.0, outer.y + 3.0));
}

#[test]
fn positioned_inline_containing_block_survives_relayout() {
    let mut harness = Harness::from_html(STRETCHED_LINK_PAGE);
    let pseudo = after_pseudo(&harness, "#sport");
    let home = harness.node("#home");

    // Hover the link (restyles it, relaying out the heading's inline content)
    // and then move away again, several times
    for _ in 0..3 {
        let (x, y) = fragment_center(&harness, "#label");
        harness.move_mouse_to(x, y);
        let (x, y) = fragment_center(&harness, "#home");
        harness.move_mouse_to(x, y);

        let fragments = fragment_rects(&harness, "#sport");
        assert_rect_eq(
            harness.layout_rect_of(pseudo),
            fragments[0],
            "::after should stay bounded to the inline after relayout",
        );
        let hovered = harness.hovered().expect("hovering #home");
        assert!(is_within(&harness, hovered, home), "hovered {hovered:?}");
    }

    // A viewport resize relays out everything from scratch
    harness.set_viewport_size(640, 480);
    let fragments = fragment_rects(&harness, "#sport");
    assert_rect_eq(
        harness.layout_rect_of(pseudo),
        fragments[0],
        "::after should stay bounded to the inline after a viewport resize",
    );
}
