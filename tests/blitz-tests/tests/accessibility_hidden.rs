use accesskit::{Node as AccessKitNode, Role};
use blitz_dom::{Attribute, BaseDocument, DocumentConfig, qual_name};
use test_that::prelude::*;

#[test]
fn includes_ordinary_div_as_node() -> TestResult<()> {
    let mut document = BaseDocument::new(DocumentConfig::default());
    let mut mutator = document.mutate();
    let html_element_id = mutator.create_element(qual_name!("html"), vec![]);
    mutator.append_children(mutator.doc.root_node().id, &[html_element_id]);
    let div_element_id = mutator.create_element(qual_name!("div"), vec![]);
    mutator.append_children(html_element_id, &[div_element_id]);
    drop(mutator);
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
    let mut document = BaseDocument::new(DocumentConfig::default());
    let mut mutator = document.mutate();
    let html_element_id = mutator.create_element(qual_name!("html"), vec![]);
    mutator.append_children(mutator.doc.root_node().id, &[html_element_id]);
    let div_element_id = mutator.create_element(
        qual_name!("div"),
        vec![Attribute {
            name: qual_name!("hidden"),
            value: String::new(),
        }],
    );
    mutator.append_children(html_element_id, &[div_element_id]);
    drop(mutator);
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
    let mut document = BaseDocument::new(DocumentConfig::default());
    let mut mutator = document.mutate();
    let html_element_id = mutator.create_element(qual_name!("html"), vec![]);
    mutator.append_children(mutator.doc.root_node().id, &[html_element_id]);
    let div_element_id = mutator.create_element(
        qual_name!("div"),
        vec![Attribute {
            name: qual_name!("style"),
            value: "display: none;".to_string(),
        }],
    );
    mutator.append_children(html_element_id, &[div_element_id]);
    drop(mutator);
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
    let mut document = BaseDocument::new(DocumentConfig::default());
    let mut mutator = document.mutate();
    let html_element_id = mutator.create_element(qual_name!("html"), vec![]);
    mutator.append_children(mutator.doc.root_node().id, &[html_element_id]);
    let div_element_id = mutator.create_element(
        qual_name!("div"),
        vec![Attribute {
            name: qual_name!("style"),
            value: "visibility: hidden;".to_string(),
        }],
    );
    mutator.append_children(html_element_id, &[div_element_id]);
    drop(mutator);
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
    let mut document = BaseDocument::new(DocumentConfig::default());
    let mut mutator = document.mutate();
    let html_element_id = mutator.create_element(qual_name!("html"), vec![]);
    mutator.append_children(mutator.doc.root_node().id, &[html_element_id]);
    let div_element_id = mutator.create_element(
        qual_name!("div"),
        vec![Attribute {
            name: qual_name!("aria-hidden"),
            value: "true".to_string(),
        }],
    );
    mutator.append_children(html_element_id, &[div_element_id]);
    drop(mutator);
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
