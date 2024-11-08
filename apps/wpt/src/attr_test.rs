use std::{sync::Arc, time::Duration};

use blitz_dom::{net::Resource, Document, HtmlDocument, Node};
use blitz_traits::net::SharedProvider;
use parley::FontContext;
use tokio::time::timeout;
use url::Url;

use crate::{clone_font_ctx, BlitzContext, TestResult};

pub async fn process_attr_test(
    font_ctx: &FontContext,
    _test_url: &Url,
    _subtest_selector: &str,
    test_file_contents: &str,
    base_url: &Url,
    blitz_context: &mut BlitzContext,
) -> TestResult {
    let mut document =
        parse_and_resolve_document(font_ctx, test_file_contents, base_url, blitz_context).await;

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
    font_ctx: &FontContext,
    test_file_contents: &str,
    base_url: &Url,
    blitz_context: &mut BlitzContext,
) -> Document {
    let mut document = HtmlDocument::from_html(
        test_file_contents,
        Some(base_url.to_string()),
        Vec::new(),
        Arc::clone(&blitz_context.net) as SharedProvider<Resource>,
        Some(clone_font_ctx(font_ctx)),
    );

    document
        .as_mut()
        .set_viewport(blitz_context.viewport.clone());

    while !blitz_context.net.is_empty() {
        let Ok(Some(res)) =
            timeout(Duration::from_millis(500), blitz_context.receiver.recv()).await
        else {
            break;
        };
        document.as_mut().load_resource(res);
    }

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
