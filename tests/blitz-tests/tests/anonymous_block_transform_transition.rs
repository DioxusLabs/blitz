//! A `transform` transition on an element inside an anonymous block box has to
//! reach its end value in the transform the painter uses, not just in the
//! computed style.
//!
//! <https://github.com/DioxusLabs/blitz/issues/1371>
//!
//! Damage marking walks the DOM parent chain, which bypasses anonymous boxes, so
//! `resolve_transforms` must not skip an anonymous box for carrying no damage of
//! its own, or the transforms beneath it are never updated.

use blitz_dom::{QualName, local_name, ns};
use blitz_test_harness::Harness;

fn page(beside_a_block: bool) -> Harness {
    let panel = if beside_a_block {
        "<div>the panel</div>"
    } else {
        ""
    };
    Harness::from_html(&format!(
        r#"<html><head><style>
            body {{ margin: 0; }}
            .chev {{ display: inline-flex; width: 20px; height: 20px; transition: transform 200ms linear; }}
            .open .chev {{ transform: rotate(180deg); }}
            #ref {{ width: 20px; height: 20px; transform: rotate(180deg); }}
        </style></head><body>
            <div id="w"><button style="display: inline-flex"><span>THREADS</span><span class="chev" id="c"></span></button>{panel}</div>
            <div id="ref"></div>
        </body></html>"#
    ))
}

fn open_and_settle(harness: &mut Harness) {
    let w = harness.node("#w");
    let class = QualName::new(None, ns!(), local_name!("class"));
    harness.base_mut().mutate().set_attribute(w, class, "open");
    for _ in 0..30 {
        harness.tick(1.0 / 60.0);
    }
}

fn transform_coeffs(harness: &Harness, selector: &str) -> Option<[f64; 6]> {
    let node_id = harness.node(selector);
    harness
        .base()
        .get_node(node_id)
        .unwrap()
        .transform()
        .as_deref()
        .map(|affine| affine.as_coeffs())
}

#[test]
fn transform_transition_reaches_end_value() {
    let mut harness = page(false);
    open_and_settle(&mut harness);
    assert_eq!(
        transform_coeffs(&harness, "#c"),
        transform_coeffs(&harness, "#ref")
    );
}

#[test]
fn transform_transition_under_anonymous_block_reaches_end_value() {
    let mut harness = page(true);
    let button = harness.node("button");
    let parent = harness.base().get_node(button).unwrap().layout_parent.get();
    assert!(
        harness
            .base()
            .get_node(parent.unwrap())
            .unwrap()
            .is_anonymous()
    );
    open_and_settle(&mut harness);
    assert_eq!(
        transform_coeffs(&harness, "#c"),
        transform_coeffs(&harness, "#ref")
    );
}
