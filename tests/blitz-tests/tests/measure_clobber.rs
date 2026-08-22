//! Regression test: a measure (ComputeSize) pass must not overwrite the stored
//! layouts of a node's children. If it does, a later cache hit on the parent's
//! PerformLayout leaves the children with measure-time geometry (observed as
//! the README image becoming too wide on github.com after a relayout).

use blitz_test_harness::Harness;
use markup5ever::{QualName, local_name, ns};
use taffy::{AvailableSpace, LayoutPartialTree as _, Size};

fn style_attr() -> QualName {
    QualName::new(None, ns!(), local_name!("style"))
}

#[test]
fn measure_pass_does_not_clobber_inline_child_layout() {
    let html = r#"
        <!DOCTYPE html>
        <html>
        <body style="margin: 0">
            <div id="other" style="height: 10px"></div>
            <p id="target" style="margin: 0">
                <img src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAADUlEQVR4nGNgYGD4DwABBAEAX+XBhAAAAABJRU5ErkJggg==" width="1600" height="200" style="max-width: 100%">
            </p>
        </body>
        </html>
    "#;
    let mut harness = Harness::from_html(html);

    let width_before = harness.layout_rect("img").width;
    assert_eq!(width_before, 800.0);

    // Measure the paragraph at a different width, as e.g. block or flexbox
    // intrinsic-height sizing does when a distant ancestor relayouts.
    let p_id = harness.query("#target").unwrap();
    let mut doc = harness.base_mut();
    doc.compute_child_layout(
        blitz_dom::taffy_node_id(p_id),
        taffy::LayoutInput {
            run_mode: taffy::RunMode::ComputeSize,
            sizing_mode: taffy::SizingMode::InherentSize,
            axis: taffy::RequestedAxis::Both,
            known_dimensions: Size {
                width: Some(500.0),
                height: None,
            },
            known_dimensions_are_definite: taffy::geometry::Size {
                width: true,
                height: false,
            },
            parent_size: Size {
                width: Some(500.0),
                height: None,
            },
            available_space: Size {
                width: AvailableSpace::Definite(500.0),
                height: AvailableSpace::MaxContent,
            },
            vertical_margins_are_collapsible: taffy::Line::FALSE,
        },
    );
    drop(doc);

    // Trigger a relayout in which the paragraph's PerformLayout is a cache hit,
    // so its children keep whatever geometry is stored for them.
    let other_id = harness.query("#other").unwrap();
    harness
        .base_mut()
        .mutate()
        .set_attribute(other_id, style_attr(), "height: 20px");
    harness.pump();

    let width_after = harness.layout_rect("img").width;
    assert_eq!(
        width_after, 800.0,
        "measure pass must not modify stored child layouts"
    );
}
