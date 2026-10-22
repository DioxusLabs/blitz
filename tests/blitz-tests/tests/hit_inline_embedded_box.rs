//! Hit testing atomic inline boxes inside a padded inline root.
//!
//! Text clusters are hit in the inline root's content box, but embedded
//! boxes store border-box relative locations like every other child. The
//! hit test subtracted the content-box offset before descending into the
//! children, so an atomic inline in a padded inline root was hit one
//! padding down and right of where it is painted: a search field built as
//! an inline-flex box inside a padded container missed clicks into its
//! upper half.

use blitz_test_harness::{Harness, HarnessOptions};

fn harness() -> Harness {
    Harness::from_html_with(
        "<html><head><style>\
         body { margin: 0; font-size: 14px; line-height: 20px }\
         #pad { padding: 10px }\
         #box { display: inline-flex; width: 200px; height: 32px }\
         #box input { width: 100%; margin: 6px }\
         </style></head><body>\
         <div id=pad><div id=box><input id=field></div>after</div>\
         </body></html>",
        HarnessOptions {
            width: 600,
            height: 400,
            ..Default::default()
        },
    )
}

fn hit_id(harness: &Harness, x: f32, y: f32) -> String {
    let node_id = harness.hit_node(x, y);
    let doc = harness.base();
    doc.get_node(node_id)
        .and_then(|n| n.data.downcast_element())
        .and_then(|el| el.id.as_ref().map(|id| id.to_string()))
        .unwrap_or_default()
}

/// The whole painted rect of the embedded box hits it, top-left corner
/// included, not just the part below and right of one padding's worth.
#[test]
fn an_embedded_box_is_hit_where_it_is_painted() {
    let harness = harness();
    let rect = harness.layout_rect("#box");

    // near the top-left corner, inside the box but within one padding of
    // its edges - the region the double-subtraction pushed out of the box
    assert_eq!(hit_id(&harness, rect.x + 2.0, rect.y + 2.0), "box");
    // dead center hits the input (margin 6 inside the box)
    assert_eq!(
        hit_id(
            &harness,
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0
        ),
        "field"
    );
    // just outside the top-left corner hits the padded container
    assert_eq!(hit_id(&harness, rect.x - 2.0, rect.y - 2.0), "pad");
    // just past the bottom-right corner, where the shifted hit region
    // used to sit
    assert_eq!(
        hit_id(
            &harness,
            rect.x + rect.width + 2.0,
            rect.y + rect.height + 2.0
        ),
        "pad"
    );
}
