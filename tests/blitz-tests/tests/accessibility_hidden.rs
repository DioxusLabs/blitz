use accesskit::{Node as AccessKitNode, Role};
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

fn default_document_config() -> DocumentConfig {
    DocumentConfig {
        viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
        html_parser_provider: Some(Arc::new(HtmlProvider) as _),
        ..Default::default()
    }
}
