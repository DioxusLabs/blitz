//! `position: absolute` boxes whose containing block is a positioned
//! non-atomic inline element (which has no layout node of its own: it is laid
//! out as a style span within its inline root's text layout) must be positioned
//! against the bounding box of that inline's line fragments (CSS 2.1 §10.1),
//! not against a further ancestor.
//!
//! Regression test for the BBC homepage: section-heading links are inline
//! `position: relative` elements with a stretched `::after { position: absolute;
//! inset: 0 }`. With the containing block falling through to the root, every
//! such `::after` covered the whole viewport and swallowed pointer events for
//! the page header.

use blitz_test_harness::{Harness, Rect};

fn assert_rect_eq(actual: Rect, expected: Rect, what: &str) {
    let close = |a: f32, b: f32| (a - b).abs() < 1.0;
    assert!(
        close(actual.x, expected.x)
            && close(actual.y, expected.y)
            && close(actual.width, expected.width)
            && close(actual.height, expected.height),
        "{what}: expected {expected:?}, got {actual:?}"
    );
}

/// Bounding box of a non-atomic inline element's line fragments, in page coordinates
fn inline_bounds(harness: &Harness, selector: &str) -> Rect {
    let doc = harness.base();
    let id = doc.query_selector(selector).unwrap().unwrap();
    let rect = doc.get_client_bounding_rect(id).unwrap();
    Rect {
        x: rect.x as f32,
        y: rect.y as f32,
        width: rect.width as f32,
        height: rect.height as f32,
    }
}

const STYLE: &str = r#"
    body { margin: 0; font-size: 16px; line-height: 20px; }
    .rel { position: relative; }
    .fill { position: absolute; top: 0; left: 0; right: 0; bottom: 0; }
"#;

#[test]
fn stretched_child_fills_inline_containing_block() {
    let html = format!(
        "<style>{STYLE}</style>\
         <div style='width: 400px; padding: 10px'>\
           before <span id='rel' class='rel'>relative text<span id='abs' class='fill'></span></span> after\
         </div>"
    );
    let harness = Harness::from_html(&html);

    let rel = inline_bounds(&harness, "#rel");
    assert!(
        rel.width > 0.0 && rel.height > 0.0,
        "inline has fragments: {rel:?}"
    );
    assert!(
        rel.x > 10.0,
        "inline is not at the start of the line: {rel:?}"
    );
    assert_rect_eq(harness.layout_rect("#abs"), rel, "stretched abspos child");
}

#[test]
fn stretched_pseudo_element_does_not_cover_unrelated_content() {
    let html = format!(
        "<style>{STYLE} a.rel::after {{ content: ''; position: absolute; inset: 0; z-index: 1; }}</style>\
         <div id='header' style='height: 50px'>header</div>\
         <h2 style='margin: 0'><a id='link' class='rel' href='#'>Heading link</a></h2>"
    );
    let harness = Harness::from_html(&html);

    let link = inline_bounds(&harness, "#link");
    let link_id = harness.node("#link");
    let after_id = harness.base().get_node(link_id).unwrap().after().unwrap();
    assert_rect_eq(
        harness.layout_rect_of(after_id),
        link,
        "stretched ::after of inline link",
    );

    // The header is not covered by the link's ::after
    let hit = harness.hit_node(200.0, 25.0);
    let header_id = harness.node("#header");
    let hit_in_header = std::iter::successors(Some(hit), |&id| {
        harness.base().get_node(id).and_then(|node| node.parent)
    })
    .any(|id| id == header_id);
    assert!(hit_in_header, "hit at (200, 25) should land in #header");
}

#[test]
fn descendant_of_inline_containing_block_via_inline_block() {
    // The abspos box is not a child of the positioned inline but a descendant
    // through an unpositioned inline-block (cf. WPT abspos-inline-007)
    let html = format!(
        "<style>{STYLE} .ib {{ display: inline-block; width: 30px; height: 10px; }}</style>\
         <div style='width: 400px'>\
           xx <span id='rel' class='rel'>text<span class='ib'>\
             <div id='tl' style='position: absolute; top: 0; left: 0; width: 10px; height: 10px'></div>\
             <div id='br' style='position: absolute; bottom: 0; right: 0; width: 10px; height: 10px'></div>\
           </span><span class='ib'></span></span> xx\
         </div>"
    );
    let harness = Harness::from_html(&html);

    let rel = inline_bounds(&harness, "#rel");
    let tl = harness.layout_rect("#tl");
    let br = harness.layout_rect("#br");
    assert_rect_eq(
        tl,
        Rect {
            x: rel.x,
            y: rel.y,
            width: 10.0,
            height: 10.0,
        },
        "top-left box",
    );
    assert_rect_eq(
        br,
        Rect {
            x: rel.x + rel.width - 10.0,
            y: rel.y + rel.height - 10.0,
            width: 10.0,
            height: 10.0,
        },
        "bottom-right box",
    );
}

#[test]
fn inline_containing_block_inside_anonymous_block() {
    // Inline content alongside a block sibling is wrapped in an anonymous
    // block, which becomes the inline root
    let html = format!(
        "<style>{STYLE}</style>\
         <div style='width: 400px'>\
           <div style='height: 30px'>block</div>\
           before <span id='rel' class='rel'>relative text<span id='abs' class='fill'></span></span> after\
         </div>"
    );
    let harness = Harness::from_html(&html);

    let rel = inline_bounds(&harness, "#rel");
    assert!(
        rel.y >= 30.0,
        "inline sits below the block sibling: {rel:?}"
    );
    assert_rect_eq(harness.layout_rect("#abs"), rel, "stretched abspos child");
}

#[test]
fn unpositioned_inline_does_not_establish_containing_block() {
    let html = format!(
        "<style>{STYLE}</style>\
         <div id='cb' class='rel' style='width: 200px; height: 100px; margin: 20px'>\
           before <span>plain text<span id='abs' class='fill'></span></span> after\
         </div>"
    );
    let harness = Harness::from_html(&html);
    assert_rect_eq(
        harness.layout_rect("#abs"),
        harness.layout_rect("#cb"),
        "abspos positioned against the block ancestor",
    );
}
