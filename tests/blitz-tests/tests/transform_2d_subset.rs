//! A `transform` list that mentions a 3d function but resolves to a matrix in
//! the two-dimensional subset must still be applied.
//!
//! <https://drafts.csswg.org/css-transforms-2/#two-dimensional-subset>
//!
//! `to_transform_3d_matrix` reports `has_3d` when a 3d transform *function*
//! appeared in the list, which is not the same question as whether the
//! resulting matrix needs 3d. Filtering on the flag dropped the whole
//! transform for `translate3d(x, y, 0)` -- the common vertical-centring idiom
//! -- and painted the element untransformed.

use blitz_test_harness::Harness;

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
fn translate3d_with_zero_z_resolves_to_a_2d_translation() {
    let harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <div id="box" style="width:100px; height:50px;
                                 transform: translate3d(60px, 20px, 0);"></div>
        </body></html>"#,
    );

    assert_eq!(
        transform_coeffs(&harness, "#box"),
        Some([1.0, 0.0, 0.0, 1.0, 60.0, 20.0]),
        "translate3d with a zero z is a 2d translation and must be applied"
    );
}

#[test]
fn translated_box_hit_tests_at_its_translated_position() {
    // The transform has to reach consumers, not just be resolved: hit testing
    // inverts it, so the box must stop answering at its untransformed position.
    let harness = Harness::from_html(
        r#"<html><body style="margin:0">
            <div id="box" style="width:100px; height:50px;
                                 transform: translate3d(120px, 0, 0);"></div>
        </body></html>"#,
    );

    let box_id = harness.node("#box");

    // x=150 is inside the translated box (120..220) and outside the original.
    assert_eq!(
        harness.hit_node(150.0, 25.0),
        box_id,
        "expected the box at its translated position"
    );

    // x=50 is inside the original box (0..100) and outside the translated one.
    assert_ne!(
        harness.hit_node(50.0, 25.0),
        box_id,
        "expected nothing at the box's untransformed position"
    );
}
