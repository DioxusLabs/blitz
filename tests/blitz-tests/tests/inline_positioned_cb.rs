use blitz_test_harness::{Harness, Rect};
use blitz_traits::node_id::NodeId;
use markup5ever::{QualName, local_name, ns};

fn style_attr() -> QualName {
    QualName::new(None, ns!(), local_name!("style"))
}

fn after_pseudo(harness: &Harness, selector: &str) -> NodeId {
    let node = harness.node(selector);
    harness
        .base()
        .get_node(node)
        .unwrap()
        .after()
        .expect("element should have an ::after pseudo-element")
}

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

fn is_within(harness: &Harness, mut node: NodeId, ancestor: NodeId) -> bool {
    loop {
        if node == ancestor {
            return true;
        }
        match harness.base().get_node(node).and_then(|node| node.parent) {
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

const BBC_PAGE: &str = include_str!("../../../examples/assets/bbc.html");

#[test]
fn stretched_link_pseudo_is_bounded_to_the_inline() {
    let harness = Harness::from_html(STRETCHED_LINK_PAGE);
    let pseudo = after_pseudo(&harness, "#sport");
    let fragment = fragment_rects(&harness, "#sport")[0];
    let actual = harness.layout_rect_of(pseudo);

    assert!(
        (actual.x - fragment.x).abs() < 1.0,
        "{actual:?} {fragment:?}"
    );
    assert!(
        (actual.width - fragment.width).abs() < 1.0,
        "{actual:?} {fragment:?}"
    );
    assert!(actual.height > 0.0 && actual.height <= fragment.height + 1.0);
    assert!(actual.y >= fragment.y - 1.0);
    assert!(actual.y + actual.height <= fragment.y + fragment.height + 1.0);
}

#[test]
fn stretched_link_pseudo_does_not_swallow_neighbor_hits() {
    let harness = Harness::from_html(STRETCHED_LINK_PAGE);
    let home = harness.node("#home");
    let news = harness.node("#news");

    for selector in ["#home", "#news"] {
        let fragment = fragment_rects(&harness, selector)[0];
        let hit = harness.hit_node(
            fragment.x + fragment.width / 2.0,
            fragment.y + fragment.height / 2.0,
        );
        let expected = if selector == "#home" { home } else { news };
        assert!(is_within(&harness, hit, expected), "hit {hit:?}");
    }
}

#[test]
fn hover_moves_between_links_with_stretched_pseudo() {
    let mut harness = Harness::from_html(STRETCHED_LINK_PAGE);
    let home = harness.node("#home");
    let news = harness.node("#news");
    let sport = harness.node("#sport");

    for (selector, expected) in [("#home", home), ("#news", news), ("#label", sport)] {
        let fragment = fragment_rects(&harness, selector)[0];
        harness.move_mouse_to(
            fragment.x + fragment.width / 2.0,
            fragment.y + fragment.height / 2.0,
        );
        let hovered = harness.hovered().expect("hovered link");
        assert!(
            is_within(&harness, hovered, expected),
            "hovered {hovered:?}"
        );
    }
}

#[test]
fn bbc_header_hover_transfers_between_links() {
    let mut harness = Harness::from_html(BBC_PAGE);
    let selectors = [
        ".ssrcss-en6he4-NavItemHoverState",
        ".ssrcss-yg7y3n-NavItemHoverState",
        ".ssrcss-ibt0yi-NavItemHoverState",
        ".ssrcss-vgg72x-NavItemHoverState",
    ];

    for _ in 0..20 {
        for selector in selectors {
            let expected = harness.node(selector);
            let rect = harness.layout_rect(selector);
            harness.move_mouse_to(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
            let hovered = harness.hovered().expect("hovered BBC header link");
            assert!(
                is_within(&harness, hovered, expected),
                "{selector}: hovered {hovered:?}"
            );

            let pseudo = after_pseudo(&harness, selector);
            let pseudo_rect = harness.layout_rect_of(pseudo);
            assert!(pseudo_rect.x >= rect.x - 1.0, "{selector}: {pseudo_rect:?}");
            assert!(
                pseudo_rect.x + pseudo_rect.width <= rect.x + rect.width + 1.0,
                "{selector}: {pseudo_rect:?} {rect:?}"
            );
        }
    }
}

#[test]
fn explicit_insets_resolve_against_positioned_inline() {
    let harness = Harness::from_html(
        r#"<!DOCTYPE html>
        <html><head><style>
            body { margin: 0; font-size: 16px; line-height: 20px; }
            p { margin: 30px; width: 300px; }
            #rel { position: relative; }
            .abs { position: absolute; width: 20px; height: 10px; }
        </style></head>
        <body><p>aaaa <span id="rel">bbbb bbbb
          <i id="origin" class="abs" style="left: 0; top: 0"></i>
          <i id="offset" class="abs" style="left: 10px; top: 5px"></i>
          <i id="br" class="abs" style="right: 0; bottom: 0"></i>
        </span></p></body></html>
        "#,
    );
    let origin = harness.layout_rect("#origin");
    let offset = harness.layout_rect("#offset");
    assert!((offset.x - origin.x - 10.0).abs() < 1.0);
    assert!((offset.y - origin.y - 5.0).abs() < 1.0);

    let br = harness.layout_rect("#br");
    let fragment = fragment_rects(&harness, "#rel")[0];
    assert!((br.x + br.width - (fragment.x + fragment.width)).abs() < 1.0);
    assert!(br.y + br.height <= fragment.y + fragment.height + 1.0);
}

#[test]
fn multiline_inline_uses_first_and_last_fragments() {
    let harness = Harness::from_html(
        r#"<!DOCTYPE html>
        <html><head><style>
            body { margin: 0; font-size: 16px; line-height: 20px; }
            p { margin: 30px; width: 100px; }
            #rel { position: relative; }
            #abs { position: absolute; inset: 0; }
        </style></head>
        <body><p>aa <span id="rel">bbbb bbbb bbbb bbbb bbbb bbbb<i id="abs"></i></span></p></body></html>
        "#,
    );
    let fragments = fragment_rects(&harness, "#rel");
    assert!(fragments.len() >= 2, "{fragments:?}");
    let first = fragments[0];
    let last = fragments[fragments.len() - 1];
    let actual = harness.layout_rect("#abs");

    assert!((actual.x - first.x).abs() < 1.0, "{actual:?} {first:?}");
    assert!(
        (actual.x + actual.width - (last.x + last.width)).abs() < 1.0,
        "{actual:?} {last:?}"
    );
    assert!(actual.y >= first.y - 1.0);
    assert!(actual.y + actual.height <= last.y + last.height + 1.0);
    assert!(actual.height > first.height);
}

#[test]
fn mixed_font_size_does_not_use_the_whole_line_height() {
    let harness = Harness::from_html(
        r#"<!DOCTYPE html>
        <html><head><style>
            body { margin: 0; line-height: 1; }
            #rel { position: relative; font-size: 10px; }
            #large { font-size: 80px; }
            #abs { position: absolute; inset: 0; }
        </style></head>
        <body><p><span id="rel">small<i id="abs"></i></span><span id="large">large</span></p></body></html>
        "#,
    );
    let actual = harness.layout_rect("#abs");
    assert!(actual.height > 5.0 && actual.height < 30.0, "{actual:?}");
}

#[test]
fn inline_padding_expands_the_positioning_area() {
    let harness = Harness::from_html(
        r#"<!DOCTYPE html>
        <html><head><style>
            body { margin: 0; font-size: 16px; line-height: 20px; }
            #rel { position: relative; padding: 5px 7px; }
            #abs { position: absolute; inset: 0; }
        </style></head>
        <body><p><span id="rel">padded<i id="abs"></i></span></p></body></html>
        "#,
    );
    let fragment = fragment_rects(&harness, "#rel")[0];
    let actual = harness.layout_rect("#abs");
    assert!((actual.x - (fragment.x - 7.0)).abs() < 1.0);
    assert!((actual.width - (fragment.width + 14.0)).abs() < 1.0);
    assert!(actual.y <= fragment.y);
    assert!(actual.y + actual.height >= fragment.y + fragment.height);
}

#[test]
fn rtl_multiline_inline_uses_directional_edges() {
    let harness = Harness::from_html(
        r#"<!DOCTYPE html>
        <html><head><style>
            body { margin: 0; font-size: 16px; line-height: 20px; }
            p { margin: 30px; width: 100px; direction: rtl; }
            #rel { position: relative; }
            #abs { position: absolute; inset: 0; }
        </style></head>
        <body><p><span id="rel">אחד שתיים שלוש ארבע חמש שש<i id="abs"></i></span> סוף</p></body></html>
        "#,
    );
    let fragments = fragment_rects(&harness, "#rel");
    assert!(fragments.len() >= 2, "{fragments:?}");
    let first = fragments[0];
    let last = fragments[fragments.len() - 1];
    let actual = harness.layout_rect("#abs");
    assert!((actual.x - last.x).abs() < 1.0, "{actual:?} {last:?}");
    assert!(
        (actual.x + actual.width - (first.x + first.width)).abs() < 1.0,
        "{actual:?} {first:?}"
    );
}

#[test]
fn auto_insets_keep_the_inline_static_position() {
    let harness = Harness::from_html(
        r#"<!DOCTYPE html>
        <html><head><style>
            body { margin: 0; font-size: 16px; line-height: 20px; }
            #rel { position: relative; }
            #abs { position: absolute; width: 10px; height: 10px; }
        </style></head>
        <body><p><span id="rel"><span id="before">before</span><i id="abs"></i>after</span></p></body></html>
        "#,
    );
    let before = fragment_rects(&harness, "#before")[0];
    let actual = harness.layout_rect("#abs");
    assert!(
        (actual.x - (before.x + before.width)).abs() < 1.0,
        "{actual:?} {before:?}"
    );
    let containing_line = fragment_rects(&harness, "#rel")[0];
    assert!(actual.y >= containing_line.y - 1.0);
    assert!(actual.y < containing_line.y + containing_line.height);
}

#[test]
fn filter_inline_claims_fixed_descendant() {
    let harness = Harness::from_html(
        r#"<!DOCTYPE html>
        <html><head><style>
            body { margin: 0; font-size: 16px; line-height: 20px; }
            #rel { filter: grayscale(0); }
            #fixed { position: fixed; inset: 0; }
        </style></head>
        <body><p><span id="rel">filtered<i id="fixed"></i></span></p></body></html>
        "#,
    );
    let fragment = fragment_rects(&harness, "#rel")[0];
    let actual = harness.layout_rect("#fixed");
    assert!((actual.x - fragment.x).abs() < 1.0);
    assert!((actual.width - fragment.width).abs() < 1.0);
    assert!(actual.height <= fragment.height + 1.0);
}

#[test]
fn abspos_falls_back_to_positioned_block_without_inline_claimant() {
    let harness = Harness::from_html(
        r#"<!DOCTYPE html>
        <html><head><style>
            body { margin: 0; font-size: 16px; line-height: 20px; }
            #outer { position: relative; margin: 50px; border: 3px solid; width: 300px; }
            #abs { position: absolute; left: 0; top: 0; width: 20px; height: 10px; }
        </style></head>
        <body><div id="outer"><p>aaaa <span>bbbb<i id="abs"></i></span></p></div></body></html>
        "#,
    );
    let outer = harness.layout_rect("#outer");
    let abs = harness.layout_rect("#abs");
    assert_eq!((abs.x, abs.y), (outer.x + 3.0, outer.y + 3.0));
}

#[test]
fn inline_claimant_changes_are_recomputed() {
    let mut harness = Harness::from_html(
        r#"<!DOCTYPE html>
        <html><head><style>
            body { margin: 0; font-size: 16px; line-height: 20px; }
            #outer { position: relative; margin: 50px; width: 300px; height: 100px; }
            #rel { display: inline; }
            #abs { position: absolute; inset: 0; }
        </style></head>
        <body><div id="outer"><span id="rel">inline<i id="abs"></i></span></div></body></html>
        "#,
    );
    let rel = harness.node("#rel");
    assert!(harness.layout_rect("#abs").width > 200.0);

    harness.base_mut().mutate().set_attribute(
        rel,
        style_attr(),
        "display: inline; position: relative",
    );
    harness.pump();
    let claimed = harness.layout_rect("#abs");
    assert!(claimed.width < 100.0, "{claimed:?}");

    harness
        .base_mut()
        .mutate()
        .set_attribute(rel, style_attr(), "display: inline");
    harness.pump();
    assert!(harness.layout_rect("#abs").width > 200.0);
}

#[test]
fn positioned_inline_containing_block_survives_viewport_resize() {
    let mut harness = Harness::from_html(
        r#"<!DOCTYPE html>
        <html><head><style>
            body { margin: 0; font-size: 16px; line-height: 20px; }
            p { width: 50%; }
            #rel { position: relative; }
            #abs { position: absolute; inset: 0; }
        </style></head>
        <body><p><span id="rel">one two three four five six seven eight nine ten<i id="abs"></i></span></p></body></html>
        "#,
    );
    let before = harness.layout_rect("#abs");
    harness.set_viewport_size(300, 300);
    let after = harness.layout_rect("#abs");
    let fragments = fragment_rects(&harness, "#rel");
    let first = fragments[0];
    let last = fragments[fragments.len() - 1];

    assert_ne!(before, after);
    assert!((after.x - first.x).abs() < 1.0);
    assert!((after.x + after.width - (last.x + last.width)).abs() < 1.0);
    assert!(after.y >= first.y - 1.0);
    assert!(after.y + after.height <= last.y + last.height + 1.0);
}

#[test]
fn hover_can_move_descendant_in_and_out_of_inline_oof_layout() {
    let mut harness = Harness::from_html(
        r#"<!DOCTYPE html>
        <html><head><style>
            body { margin: 0; font-size: 16px; line-height: 20px; }
            #rel { position: relative; }
            #rel:hover #child { position: absolute; inset: 0; }
        </style></head>
        <body><p><span id="rel">before<i id="child">child</i>after</span></p></body></html>
        "#,
    );
    let fragment = fragment_rects(&harness, "#rel")[0];

    harness
        .base_mut()
        .set_hover_to(fragment.x + 2.0, fragment.y + 2.0);
    harness.pump();
    let out_of_flow = harness.layout_rect("#child");
    let out_of_flow_fragment = fragment_rects(&harness, "#rel")[0];
    assert!((out_of_flow.x - out_of_flow_fragment.x).abs() < 1.0);
    assert!(
        (out_of_flow.width - out_of_flow_fragment.width).abs() < 1.0,
        "{out_of_flow:?} {out_of_flow_fragment:?}"
    );

    harness.base_mut().set_hover_to(700.0, 500.0);
    harness.pump();
    let restored = fragment_rects(&harness, "#rel")[0];
    assert!((restored.x - fragment.x).abs() < 1.0);
    assert!((restored.width - fragment.width).abs() < 1.0);
}

fn assert_hover_toggles_inline_fixed_claimant(containing_block_style: &str) {
    let html = format!(
        r#"<!DOCTYPE html>
        <html><head><style>
            body {{ margin: 0; font-size: 16px; line-height: 20px; }}
            #outer {{ margin: 50px; width: 400px; }}
            #rel:hover {{ {containing_block_style} }}
            #fixed {{ position: fixed; left: 200px; top: 10px; width: 10px; height: 10px; }}
        </style></head>
        <body><div id="outer">prefix prefix <span id="rel">target<i id="fixed"></i></span></div></body></html>
        "#
    );
    let mut harness = Harness::from_html(&html);
    assert!((harness.layout_rect("#fixed").x - 200.0).abs() < 1.0);

    let fragment = fragment_rects(&harness, "#rel")[0];
    harness.base_mut().set_hover_to(
        fragment.x + fragment.width / 2.0,
        fragment.y + fragment.height / 2.0,
    );
    harness.pump();
    assert!(
        (harness.layout_rect("#fixed").x - (fragment.x + 200.0)).abs() < 1.0,
        "{}: {:?}",
        containing_block_style,
        harness.layout_rect("#fixed")
    );

    harness.base_mut().set_hover_to(700.0, 500.0);
    harness.pump();
    assert!(
        (harness.layout_rect("#fixed").x - 200.0).abs() < 1.0,
        "{}: {:?}",
        containing_block_style,
        harness.layout_rect("#fixed")
    );
}

#[test]
fn hover_filter_toggles_inline_fixed_claimant() {
    assert_hover_toggles_inline_fixed_claimant("filter: grayscale(0)");
}

#[test]
fn hover_will_change_toggles_inline_fixed_claimant() {
    assert_hover_toggles_inline_fixed_claimant("will-change: filter");
}
