use accesskit::{Node as AccessKitNode, NodeId, Role};
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;
use test_that::prelude::*;

#[test]
fn includes_ordinary_div_as_node() -> TestResult<()> {
    let mut document =
        HtmlDocument::from_html("<html><div></div></html>", default_document_config());
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::GenericContainer),
                is_hidden(): eq(false),
            })
        ))
    )
}

#[test]
fn excludes_div_with_hidden_attribute() -> TestResult<()> {
    let mut document =
        HtmlDocument::from_html("<html><div hidden></div></html>", default_document_config());
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        not(contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::GenericContainer)
            })
        )))
    )
}

#[test]
fn excludes_div_with_display_none() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><div style="display: none;"></div></html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        not(contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::GenericContainer)
            })
        )))
    )
}

#[test]
fn excludes_div_with_visibility_hidden() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><div style="visibility: hidden;"></div></html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        not(contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::GenericContainer)
            })
        )))
    )
}

#[test]
fn excludes_button_with_visibility_collapse() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><button id="target" style="visibility: collapse;"></button></html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();
    let target = NodeId(document.get_element_by_id("target").unwrap().as_u64());

    verify_that!(tree_update.nodes, not(contains((eq(target), anything()))))
}

#[test]
fn sets_hidden_flag_on_element_with_aria_hidden_attribute() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><div aria-hidden="true"></div></html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::GenericContainer),
                is_hidden(): eq(true),
            })
        ))
    )
}

#[test]
fn excludes_child_element_of_hidden_element() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <head/>
          <body>
            <div hidden="true">
              <button/>
            </div>
          </body>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        not(contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::Button),
            })
        )))
    )
}

#[test]
fn includes_visible_child_of_visibility_hidden_element() {
    let mut document = HtmlDocument::from_html(
        r#"<html><body>
          <div id="hidden" style="visibility: hidden;">
            Hidden text
            <button id="target" style="visibility: visible;">Shown</button>
          </div>
        </body></html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();
    let id = |dom_id| NodeId(document.get_element_by_id(dom_id).unwrap().as_u64());
    let node = |id| tree_update.nodes.iter().find(|(n, _)| *n == id);
    let body = NodeId(document.query_selector("body").unwrap().unwrap().as_u64());

    assert!(node(id("hidden")).is_none());
    assert!(node(id("target")).is_some());
    // The visible child is attached to its nearest ancestor in the tree.
    assert!(node(body).unwrap().1.children().contains(&id("target")));
    // Text of the hidden element stays hidden.
    assert!(
        !tree_update
            .nodes
            .iter()
            .any(|(_, n)| n.value().is_some_and(|v| v.contains("Hidden text")))
    );
}

#[test]
fn excludes_visible_child_of_aria_hidden_visibility_hidden_element() {
    let mut document = HtmlDocument::from_html(
        r#"<html><body>
          <div aria-hidden="true" style="visibility: hidden;">
            <button id="target" style="visibility: visible;">Shown</button>
          </div>
        </body></html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();
    let target = NodeId(document.get_element_by_id("target").unwrap().as_u64());

    assert!(!tree_update.nodes.iter().any(|(id, _)| *id == target));
}

fn default_document_config() -> DocumentConfig {
    DocumentConfig {
        viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
        html_parser_provider: Some(Arc::new(HtmlProvider) as _),
        ..Default::default()
    }
}
