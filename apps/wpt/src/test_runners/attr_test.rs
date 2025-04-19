use blitz_dom::Node;

use super::{SubtestResult, parse_and_resolve_document};
use crate::{SubtestCounts, TestStatus, ThreadCtx};

fn status_from_bool(input: bool) -> TestStatus {
    if input {
        TestStatus::Pass
    } else {
        TestStatus::Fail
    }
}

pub fn process_attr_test(
    ctx: &mut ThreadCtx,
    subtest_selector: &str,
    html: &str,
    relative_path: &str,
) -> (SubtestCounts, Vec<SubtestResult>) {
    let mut document = parse_and_resolve_document(ctx, html, relative_path);

    let Ok(subtest_roots) = document.query_selector_all(subtest_selector) else {
        panic!("Err parsing subtest selector \"{}\"", subtest_selector);
    };
    if subtest_roots.is_empty() {
        panic!(
            "No matching nodes found for subtest selector \"{}\"",
            subtest_selector
        );
    }

    let subtest_count = subtest_roots.len() as u32;
    let mut pass_count: u32 = 0;
    let mut fail_count: u32 = 0;

    let subtest_results: Vec<_> = subtest_roots
        .into_iter()
        .enumerate()
        .map(|(idx, root_id)| {
            let mut has_error = false;
            document.iter_subtree_mut(root_id, |node_id, doc| {
                let node = doc.get_node(node_id).unwrap();
                let passes = check_node_layout(node);
                has_error |= !passes;
            });

            if has_error {
                fail_count += 1;
            } else {
                pass_count += 1;
            }

            SubtestResult {
                name: format!("{subtest_selector} {}", idx + 1),
                status: status_from_bool(!has_error),
                message: None, // TODO: error message
            }
        })
        .collect();

    assert!(pass_count + fail_count == subtest_count);
    let subtest_counts = SubtestCounts {
        pass: pass_count,
        total: subtest_count,
    };

    (subtest_counts, subtest_results)
}

pub fn check_node_layout(node: &Node) -> bool {
    let layout = &node.final_layout;
    let parent_border = if let Some(parent_id) = node.parent {
        node.with(parent_id).final_layout.border
    } else {
        taffy::Rect::ZERO
    };

    node.attrs()
        .map(|attrs| {
            attrs.iter().all(|attr| {
                let name = attr.name.local.as_ref();
                let value = &attr.value;
                match name {
                    "data-expected-width" => assert_with_tolerance(value, layout.size.width),
                    "data-expected-height" => assert_with_tolerance(value, layout.size.height),
                    "data-expected-padding-top" => assert_with_tolerance(value, layout.padding.top),
                    "data-expected-padding-bottom" => {
                        assert_with_tolerance(value, layout.padding.bottom)
                    }
                    "data-expected-padding-left" => {
                        assert_with_tolerance(value, layout.padding.left)
                    }
                    "data-expected-padding-right" => {
                        assert_with_tolerance(value, layout.padding.right)
                    }
                    "data-expected-margin-top" => assert_with_tolerance(value, layout.margin.top),
                    "data-expected-margin-bottom" => {
                        assert_with_tolerance(value, layout.margin.bottom)
                    }
                    "data-expected-margin-left" => assert_with_tolerance(value, layout.margin.left),
                    "data-expected-margin-right" => {
                        assert_with_tolerance(value, layout.margin.right)
                    }

                    // TODO: Implement proper offset-x/offset-y computation
                    // (don't assume that offset is relative to immediate parent)
                    "data-offset-x" => {
                        assert_with_tolerance(value, layout.location.x - parent_border.left)
                    }
                    "data-offset-y" => {
                        assert_with_tolerance(value, layout.location.y - parent_border.top)
                    }

                    // TODO: other check types
                    "data-expected-client-width" => false,
                    "data-expected-client-height" => false,
                    "data-expected-scroll-width" => false,
                    "data-expected-scroll-height" => false,
                    "data-expected-bounding-client-rect-width" => false,
                    "data-expected-bounding-client-rect-height" => false,
                    "data-total-x" => false,
                    "data-total-y" => false,
                    "data-expected-display" => false,

                    // Not a check attribute
                    _ => true,
                }
            })
        })
        .unwrap_or(true)
}

fn assert_with_tolerance(attr_val: &str, actual: f32) -> bool {
    let expected: f32 = attr_val
        .parse()
        .expect("Failed to parse check attribute as f32");
    (actual - expected).abs() < 1.0
}
