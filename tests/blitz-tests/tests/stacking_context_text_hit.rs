//! A stacking-context root is descended into during hit testing even when the
//! point lies outside its own box, because one of its stacked entries may be
//! positioned anywhere. The root's *inline text* however never lies outside
//! the root's box/overflow area, so text hit-testing must only run for points
//! that do: Parley's line lookup clamps out-of-range block offsets to the
//! first/last line, so an unguarded text hit-test reports glyph hits for
//! points arbitrarily far above or below the inline root.
//!
//! Pattern from html.spec.whatwg.org: `h4 { position: relative; z-index: 3 }`
//! with an absolutely-positioned self-link child, thousands of pixels below
//! the table of contents.

use blitz_test_harness::Harness;

const PAGE: &str = r##"<!DOCTYPE html>
<html><head><style>
    body { margin: 0; font-size: 16px; line-height: 20px; }
    #toc { padding: 10px 40px; }
    #spacer { height: 3000px; }
    h4 { position: relative; z-index: 3; margin: 0 40px; }
    h4 .self-link { position: absolute; top: 0; left: -30px; width: 20px; height: 20px; }
</style></head>
<body>
    <div id="toc"><a id="toc-link" href="#heading">Table of contents link</a></div>
    <div id="spacer"></div>
    <h4 id="heading"><span class="secno">1.2.3</span> Heading text <a class="self-link" href="#heading"></a></h4>
</body></html>"##;

#[test]
fn text_outside_positioned_heading_does_not_hit_heading() {
    let mut harness = Harness::from_html(PAGE);
    let link = harness.node("#toc-link");
    let rect = harness
        .base()
        .inline_fragment_rects(link)
        .expect("link is a non-atomic inline")[0];
    let (x, y) = (
        (rect.x + rect.width / 2.0) as f32,
        (rect.y + rect.height / 2.0) as f32,
    );

    assert_eq!(harness.hit_node(x, y), link);

    harness.move_mouse_to(x, y);
    assert_eq!(harness.hovered(), Some(link));

    // Empty space beside/below the link must not hit the heading's text either
    assert_eq!(harness.hit_node(600.0, y), harness.node("#toc"));
    assert_eq!(harness.hit_node(x, 200.0), harness.node("#spacer"));

    // The heading itself (and its hoisted self-link) still hit-test normally
    let heading = harness.node("h4");
    let heading_rect = harness.layout_rect("h4");
    let (hx, hy) = heading_rect.center();
    let hit = harness.hit_node(hx, hy);
    assert!(
        hit == heading || hit == harness.node(".secno"),
        "expected heading text hit, got {hit:?}"
    );
    let self_link = harness.node(".self-link");
    let self_link_rect = harness.layout_rect_of(self_link);
    let (sx, sy) = self_link_rect.center();
    assert_eq!(harness.hit_node(sx, sy), self_link);
}
