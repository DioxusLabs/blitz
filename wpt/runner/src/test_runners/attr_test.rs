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
) -> (TestStatus, SubtestCounts, Vec<SubtestResult>) {
    let mut document = parse_and_resolve_document(ctx, html, relative_path);

    let Ok(subtest_roots) = document.query_selector_all(subtest_selector) else {
        panic!("Err parsing subtest selector \"{subtest_selector}\"");
    };
    if subtest_roots.is_empty() {
        println!("No matching nodes found for subtest selector \"{subtest_selector}\"");
        return (TestStatus::Fail, SubtestCounts::ZERO_OF_ZERO, Vec::new());
    }

    let subtest_count = subtest_roots.len() as u32;
    let mut pass_count: u32 = 0;
    let mut fail_count: u32 = 0;

    let subtest_results: Vec<_> = subtest_roots
        .into_iter()
        .enumerate()
        .map(|(idx, root_id)| {
            let mut errors = Vec::new();
            document.iter_subtree_mut(root_id, |node_id, doc| {
                let node = doc.get_node(node_id).unwrap();
                errors.extend_from_slice(&check_node_layout(node));
            });

            let has_error = !errors.is_empty();
            if has_error {
                fail_count += 1;
            } else {
                pass_count += 1;
            }

            SubtestResult {
                name: format!("{subtest_selector} {}", idx + 1),
                status: status_from_bool(!has_error),
                errors,
            }
        })
        .collect();

    assert!(pass_count + fail_count == subtest_count);
    let subtest_counts = SubtestCounts {
        pass: pass_count,
        total: subtest_count,
    };

    let status = subtest_counts.as_status();
    (status, subtest_counts, subtest_results)
}

pub fn check_node_layout(node: &Node) -> Vec<String> {
    let layout = &node.final_layout;
    let parent_border = if let Some(parent_id) = node.parent {
        node.with(parent_id).final_layout.border
    } else {
        taffy::Rect::ZERO
    };

    node.attrs()
        .map(|attrs| {
            attrs
                .iter()
                .map(|attr| {
                    let name = attr.name.local.as_ref();
                    let value = &attr.value;
                    match name {
                        "data-expected-width" => check_attr(name, value, layout.size.width),
                        "data-expected-height" => check_attr(name, value, layout.size.height),
                        "data-expected-padding-top" => check_attr(name, value, layout.padding.top),
                        "data-expected-padding-bottom" => {
                            check_attr(name, value, layout.padding.bottom)
                        }
                        "data-expected-padding-left" => {
                            check_attr(name, value, layout.padding.left)
                        }
                        "data-expected-padding-right" => {
                            check_attr(name, value, layout.padding.right)
                        }
                        "data-expected-margin-top" => check_attr(name, value, layout.margin.top),
                        "data-expected-margin-bottom" => {
                            check_attr(name, value, layout.margin.bottom)
                        }
                        "data-expected-margin-left" => check_attr(name, value, layout.margin.left),
                        "data-expected-margin-right" => {
                            check_attr(name, value, layout.margin.right)
                        }

                        // TODO: Implement proper offset-x/offset-y computation
                        // (don't assume that offset is relative to immediate parent)
                        "data-offset-x" => {
                            check_attr(name, value, layout.location.x - parent_border.left)
                        }
                        "data-offset-y" => {
                            check_attr(name, value, layout.location.y - parent_border.top)
                        }

                        // TODO: other check types
                        "data-expected-client-width" => {
                            Err(format!("Unsupported assertion: {name}"))
                        }
                        "data-expected-client-height" => {
                            Err(format!("Unsupported assertion: {name}"))
                        }
                        "data-expected-scroll-width" => {
                            Err(format!("Unsupported assertion: {name}"))
                        }
                        "data-expected-scroll-height" => {
                            Err(format!("Unsupported assertion: {name}"))
                        }
                        "data-expected-bounding-client-rect-width" => {
                            Err(format!("Unsupported assertion: {name}"))
                        }
                        "data-expected-bounding-client-rect-height" => {
                            Err(format!("Unsupported assertion: {name}"))
                        }
                        "data-total-x" => Err(format!("Unsupported assertion: {name}")),
                        "data-total-y" => Err(format!("Unsupported assertion: {name}")),
                        "data-expected-display" => Err(format!("Unsupported assertion: {name}")),

                        // Not a check attribute
                        _ => Ok(()),
                    }
                })
                .filter_map(|result| result.err())
                .collect()
        })
        .unwrap_or_default()
}

fn check_attr(attr_name: &str, attr_val: &str, actual: f32) -> Result<(), String> {
    let expected: f32 = attr_val
        .parse()
        .expect("Failed to parse check attribute as f32");

    let equal = assert_with_tolerance(expected, actual);

    match equal {
        true => Ok(()),
        false => Err(format!(
            "assert_equals: {attr_name} expected {expected} got {actual}"
        )),
    }
}

fn assert_with_tolerance(expected: f32, actual: f32) -> bool {
    (actual - expected).abs() < 1.0
}
