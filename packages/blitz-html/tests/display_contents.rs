//! display:contents — children must lay out as if they were children of the
//! contents element's parent.

use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn layout_doc(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

#[test]
fn contents_block_children_stack_and_fill_width() {
    let doc = layout_doc(
        r#"<html><body style="margin:0">
            <div style="width:300px;">
                <div style="display:contents;">
                    <div id="a" style="height:50px;"></div>
                    <div id="b" style="height:70px;"></div>
                </div>
            </div>
        </body></html>"#,
    );
    let a = doc.query_selector("#a").unwrap().expect("#a not found");
    let b = doc.query_selector("#b").unwrap().expect("#b not found");
    let la = doc.get_node(a).unwrap().final_layout;
    let lb = doc.get_node(b).unwrap().final_layout;
    assert_eq!(
        (la.size.width, la.size.height),
        (300.0, 50.0),
        "first hoisted block child must fill the grandparent width"
    );
    assert_eq!(
        (lb.size.width, lb.size.height),
        (300.0, 70.0),
        "second hoisted block child must fill the grandparent width"
    );
    assert_eq!(
        lb.location.y - la.location.y,
        50.0,
        "hoisted block children must stack vertically"
    );
}

#[test]
fn scroller_wrapping_contents_gets_full_scroll_size() {
    // The kopuz route-shell shape: a scroll container whose only child is a
    // display:contents wrapper holding tall content.
    let doc = layout_doc(
        r#"<html><body style="margin:0">
            <div id="scroller" style="height:200px; overflow-y:auto;">
                <div style="display:contents;">
                    <div id="tall" style="height:1000px;"></div>
                </div>
            </div>
        </body></html>"#,
    );
    let scroller = doc
        .query_selector("#scroller")
        .unwrap()
        .expect("#scroller not found");
    let tall = doc.query_selector("#tall").unwrap().expect("#tall not found");
    let ls = doc.get_node(scroller).unwrap().final_layout;
    let lt = doc.get_node(tall).unwrap().final_layout;
    assert_eq!(
        (lt.size.width, lt.size.height),
        (800.0, 1000.0),
        "content inside the contents wrapper must size normally"
    );
    assert_eq!(
        ls.content_size.height, 1000.0,
        "the scroller's scrollable content size must include the hoisted content"
    );
}

#[test]
fn contents_in_flex_container_hoists_children_as_flex_items() {
    // The kopuz showcase shape: flex rows whose direct children are
    // display:contents wrappers (keyed row wrappers).
    let doc = layout_doc(
        r#"<html><body style="margin:0">
            <div style="display:flex; flex-direction:column; width:300px;">
                <div style="display:contents;">
                    <div id="x" style="height:40px;"></div>
                    <div id="y" style="height:40px;"></div>
                </div>
            </div>
        </body></html>"#,
    );
    let x = doc.query_selector("#x").unwrap().expect("#x not found");
    let y = doc.query_selector("#y").unwrap().expect("#y not found");
    let lx = doc.get_node(x).unwrap().final_layout;
    let ly = doc.get_node(y).unwrap().final_layout;
    assert_eq!(
        (lx.size.width, lx.size.height),
        (300.0, 40.0),
        "hoisted flex item must stretch to the flex container width"
    );
    assert_eq!(
        ly.location.y - lx.location.y,
        40.0,
        "hoisted flex items must stack in the column"
    );
}

