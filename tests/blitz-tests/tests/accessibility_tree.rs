//! The structure of the accessibility tree as a whole, as opposed to the properties of its nodes.

use accesskit::{Node as AccessKitNode, Role, TreeUpdate};
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;
use test_that::prelude::*;

#[test]
fn includes_the_root_of_the_tree_as_a_node_with_window_role() -> TestResult<()> {
    let mut document =
        HtmlDocument::from_html("<html><div></div></html>", default_document_config());
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    let root = root_node_id(&tree_update)?;
    verify_that!(
        tree_update.nodes,
        contains((
            eq(root),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::Window),
            })
        ))
    )
}

#[test]
fn attaches_the_content_of_the_document_to_the_root_of_the_tree() -> TestResult<()> {
    let mut document =
        HtmlDocument::from_html("<html><div></div></html>", default_document_config());
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    let root = root_node_id(&tree_update)?;
    verify_that!(
        tree_update.nodes,
        contains((
            eq(root),
            matches_pattern!(AccessKitNode {
                *children(): not(is_empty()),
            })
        ))
    )
}

/// The ID which the tree names as its root.
fn root_node_id(tree_update: &TreeUpdate) -> TestResult<accesskit::NodeId> {
    let Some(tree) = &tree_update.tree else {
        fail!("Expected the update to describe the tree, but it does not")?;
        unreachable!();
    };
    Ok(tree.root)
}

fn default_document_config() -> DocumentConfig {
    DocumentConfig {
        viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
        html_parser_provider: Some(Arc::new(HtmlProvider) as _),
        ..Default::default()
    }
}
