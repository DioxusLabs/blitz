use accesskit::{Node as AccessKitNode, Role};
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;
use test_that::prelude::*;

#[test]
fn includes_aria_description_as_description() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><div role="button" aria-description="A description"></div></html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::Button),
                description(): some(eq("A description")),
            })
        ))
    )
}

#[test]
fn includes_aria_describedby_targets_as_described_by_in_order() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <div role="button" id="self" aria-describedby="second self nonexistent first"></div>
          <span id="first">First</span>
          <p id="second">Second</p>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    let node_id_with = |predicate: fn(&AccessKitNode) -> bool| {
        tree_update
            .nodes
            .iter()
            .find(|(_, node)| predicate(node))
            .expect("Node must be present in accessibility tree")
            .0
    };
    let expected_node_ids = vec![
        node_id_with(|node| node.html_tag() == Some("p")),
        node_id_with(|node| node.role() == Role::Button),
        node_id_with(|node| node.html_tag() == Some("span")),
    ];
    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::Button),
                *described_by(): container_eq(expected_node_ids),
            })
        ))
    )
}

#[test]
fn excludes_aria_describedby_targets_which_are_not_in_the_accessibility_tree() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <div role="button" aria-describedby="hidden-element hidden-descendant"></div>
          <span id="hidden-element" style="display: none">Hidden</span>
          <div style="visibility: hidden"><span id="hidden-descendant">Hidden</span></div>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::Button),
                *described_by(): is_empty(),
            })
        ))
    )
}

fn default_document_config() -> DocumentConfig {
    DocumentConfig {
        viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
        html_parser_provider: Some(Arc::new(HtmlProvider) as _),
        ..Default::default()
    }
}
