use blitz_dom::Node;
use log::warn;
use style_traits::ToCss;

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
        warn!("No matching nodes found for subtest selector \"{subtest_selector}\"");
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
    if node.element_data().is_none() {
        return Vec::new();
    }
    let layout = node.final_layout();

    let client_width =
        layout.size.width - layout.border.left - layout.border.right - layout.scrollbar_size.width;
    let client_height = layout.size.height
        - layout.border.top
        - layout.border.bottom
        - layout.scrollbar_size.height;

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

                        "data-offset-x" => check_attr(name, value, node.offset_top_left().x),
                        "data-offset-y" => check_attr(name, value, node.offset_top_left().y),

                        "data-expected-client-width" => check_attr(name, value, client_width),
                        "data-expected-client-height" => check_attr(name, value, client_height),
                        "data-expected-scroll-width" => {
                            check_attr(name, value, client_width.max(layout.content_size.width))
                        }
                        "data-expected-scroll-height" => {
                            check_attr(name, value, client_height.max(layout.content_size.height))
                        }
                        "data-expected-bounding-client-rect-width" => {
                            check_attr(name, value, layout.size.width)
                        }
                        "data-expected-bounding-client-rect-height" => {
                            check_attr(name, value, layout.size.height)
                        }
                        "data-total-x" => check_attr(name, value, total_offset(node).0),
                        "data-total-y" => check_attr(name, value, total_offset(node).1),
                        "data-expected-display" => {
                            let display = node
                                .primary_styles()
                                .map(|styles| styles.clone_display().to_css_string())
                                .unwrap_or_default();
                            if display == **value {
                                Ok(())
                            } else {
                                Err(format!(
                                    "assert_equals: {name} expected {value} got {display}"
                                ))
                            }
                        }

                        // Not a check attribute
                        _ => Ok(()),
                    }
                })
                .filter_map(|result| result.err())
                .collect()
        })
        .unwrap_or_default()
}

fn total_offset(node: &Node) -> (f32, f32) {
    let mut x = 0.0;
    let mut y = 0.0;
    let mut current = node;
    loop {
        let layout = current.final_layout();
        x += layout.location.x;
        y += layout.location.y;
        match current.layout_parent.get() {
            Some(parent_id) => current = current.with(parent_id),
            None => break,
        }
    }
    (x, y)
}

fn check_attr(attr_name: &str, attr_val: &str, actual: f32) -> Result<(), String> {
    let Ok(expected) = attr_val.parse::<f32>() else {
        return Err(format!(
            "assert_equals: failed to parse {attr_name} value {attr_val} as f32"
        ));
    };

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
