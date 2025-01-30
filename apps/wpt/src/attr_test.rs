use std::sync::Arc;

use blitz_dom::{net::Resource, BaseDocument, Node};
use blitz_html::HtmlDocument;
use blitz_traits::net::SharedProvider;

use crate::{SubtestCounts, ThreadCtx};

pub async fn process_attr_test(
    ctx: &mut ThreadCtx,
    subtest_selector: &str,
    html: &str,
    relative_path: &str,
) -> SubtestCounts {
    let mut document = parse_and_resolve_document(ctx, html, relative_path).await;

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

    for root_id in subtest_roots {
        let mut has_error = false;
        document.iter_subtree_mut(root_id, |node_id, doc| {
            let node = doc.get_node(node_id).unwrap();
            let passes = check_node_layout(node);
            has_error |= !passes;
        });

        if !has_error {
            fail_count += 1;
        } else {
            pass_count += 1;
        }
    }

    assert!(pass_count + fail_count == subtest_count);
    SubtestCounts {
        pass: pass_count,
        total: subtest_count,
    }
}

pub async fn parse_and_resolve_document(
    ctx: &mut ThreadCtx,
    html: &str,
    relative_path: &str,
) -> BaseDocument {
    let mut document = HtmlDocument::from_html(
        html,
        Some(ctx.dummy_base_url.join(relative_path).unwrap().to_string()),
        Vec::new(),
        Arc::clone(&ctx.net_provider) as SharedProvider<Resource>,
        Some(ctx.font_ctx.clone()),
        ctx.navigation_provider.clone(),
    );

    document.as_mut().set_viewport(ctx.viewport.clone());

    // Load resources
    ctx.net_provider
        .for_each(|res| document.as_mut().load_resource(res));

    // Compute style, layout, etc for HtmlDocument
    document.as_mut().resolve();

    document.into()
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
