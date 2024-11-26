use std::sync::Arc;

use blitz_dom::{net::Resource, Document, Node};
use blitz_html::HtmlDocument;
use blitz_traits::net::SharedProvider;

use crate::{clone_font_ctx, TestResult, ThreadCtx};

pub async fn process_attr_test(
    ctx: &mut ThreadCtx,
    _subtest_selector: &str,
    html: &str,
    relative_path: &str,
) -> TestResult {
    let mut document = parse_and_resolve_document(ctx, html, relative_path).await;

    let mut has_error = false;
    let root_id = document.root_node().id;
    document.iter_subtree_mut(root_id, |node_id, doc| {
        let node = doc.get_node(node_id).unwrap();
        let passes = check_node_layout(node);
        has_error |= !passes;
    });

    if has_error {
        TestResult::Fail
    } else {
        TestResult::Pass
    }
}

pub async fn parse_and_resolve_document(
    ctx: &mut ThreadCtx,
    html: &str,
    relative_path: &str,
) -> Document {
    let mut document = HtmlDocument::from_html(
        html,
        Some(ctx.dummy_base_url.join(relative_path).unwrap().to_string()),
        Vec::new(),
        Arc::clone(&ctx.net_provider) as SharedProvider<Resource>,
        Some(clone_font_ctx(&ctx.font_ctx)),
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
                    "data-offset-x" => assert_with_tolerance(value, layout.location.x),
                    "data-offset-y" => assert_with_tolerance(value, layout.location.y),

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
