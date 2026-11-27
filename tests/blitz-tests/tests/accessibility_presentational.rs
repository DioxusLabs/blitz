//! Elements with a presentational role, `none` or `presentation`, are not exposed in the
//! accessibility tree. Their content is.
//!
//! The tests accept either of two ways of not exposing an element: the tree leaves the element
//! out and attaches its children to the closest element which it exposes, or the element only has
//! the role [Role::GenericContainer], which carries no semantics.
//!
//! <https://www.w3.org/TR/wai-aria-1.2/#conflict_resolution_presentation_none>

use accesskit::{Node as AccessKitNode, NodeId, Role, TreeUpdate};
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use rstest::rstest;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use test_that::prelude::*;

#[rstest]
#[case::heading_with_none(r#"<html><h1 role="none">Text</h1></html>"#, "h1")]
#[case::heading_with_presentation(r#"<html><h1 role="presentation">Text</h1></html>"#, "h1")]
#[case::paragraph(r#"<html><p role="none">Text</p></html>"#, "p")]
#[case::article(
    r#"<html><article role="presentation">Text</article></html>"#,
    "article"
)]
#[case::navigation(r#"<html><nav role="none"><p>Text</p></nav></html>"#, "nav")]
#[case::img_with_alt(r#"<html><img alt="Alt text" role="none"></html>"#, "img")]
#[case::list_with_none_and_presentation(
    r#"<html><h1 role="none presentation">Text</h1></html>"#,
    "h1"
)]
#[case::list_with_presentation_and_none(
    r#"<html><h1 role="presentation none">Text</h1></html>"#,
    "h1"
)]
#[case::list_starting_with_unknown_role(
    r#"<html><h1 role="arbitrary-unknown-role none">Text</h1></html>"#,
    "h1"
)]
#[case::upper_case_none(r#"<html><h1 role="NONE">Text</h1></html>"#, "h1")]
#[case::mixed_case_presentation(r#"<html><h1 role="Presentation">Text</h1></html>"#, "h1")]
fn does_not_expose_element_with_presentational_role(
    #[case] html: &str,
    #[case] html_tag: &str,
) -> TestResult<()> {
    let tree_update = tree_of(html);

    verify_not_exposed(&tree_update, html_tag)
}

/// Not exposing an element does not remove its content from the tree.
#[rstest]
#[case::heading(r#"<html><h1 role="none">Arbitrary text</h1></html>"#)]
#[case::list(r#"<html><ul role="presentation"><li>Arbitrary text</li></ul></html>"#)]
#[case::table(r#"<html><table role="none"><tr><td>Arbitrary text</td></tr></table></html>"#)]
#[case::link_in_navigation(
    r##"<html><nav role="none"><a href="#target">Arbitrary text</a></nav></html>"##
)]
fn keeps_the_content_of_element_with_presentational_role_in_the_tree(
    #[case] html: &str,
) -> TestResult<()> {
    let tree_update = tree_of(html);

    let reachable = reachable_node_ids(&tree_update);
    let reachable_text_runs = tree_update
        .nodes
        .iter()
        .filter(|(id, node)| {
            node.role() == Role::TextRun
                && node.value() == Some("Arbitrary text")
                && reachable.contains(id)
        })
        .count();
    verify_that!(reachable_text_runs, eq(1))
}

/// An element which is focusable or otherwise interactive keeps its implicit role, so that
/// the user can operate it.
#[rstest]
#[case::button(
    r#"<html><button role="presentation">Text</button></html>"#,
    "button",
    Role::Button
)]
#[case::link(
    r##"<html><a href="#target" role="none">Text</a></html>"##,
    "a",
    Role::Link
)]
#[case::text_input(
    r#"<html><input role="presentation"></html>"#,
    "input",
    Role::TextInput
)]
#[case::select(
    r#"<html><select role="none"><option>One</option></select></html>"#,
    "select",
    Role::ComboBox
)]
#[case::textarea(
    r#"<html><textarea role="none"></textarea></html>"#,
    "textarea",
    Role::MultilineTextInput
)]
#[case::focusable_heading(
    r#"<html><h1 role="none" tabindex="0">Text</h1></html>"#,
    "h1",
    Role::Heading
)]
fn ignores_presentational_role_of_interactive_element(
    #[case] html: &str,
    #[case] html_tag: &str,
    #[case] role: Role,
) -> TestResult<()> {
    let tree_update = tree_of(html);

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                html_tag(): some(eq(html_tag)),
                role(): eq(role),
            })
        ))
    )
}

/// An element with a global state or property keeps its implicit role.
#[rstest]
#[case::aria_describedby(
    r#"<html><h1 role="presentation" aria-describedby="arbitrary-element">Text</h1><span id="arbitrary-element">Description</span></html>"#
)]
#[case::aria_label(
    r#"<html><h1 role="presentation" aria-label="Arbitrary label">Text</h1></html>"#
)]
#[case::aria_labelledby(
    r#"<html><h1 role="presentation" aria-labelledby="arbitrary-element">Text</h1><span id="arbitrary-element">Label</span></html>"#
)]
#[case::aria_live(r#"<html><h1 role="presentation" aria-live="polite">Text</h1></html>"#)]
fn ignores_presentational_role_of_element_with_global_aria_attribute(
    #[case] html: &str,
) -> TestResult<()> {
    let tree_update = tree_of(html);

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                html_tag(): some(eq("h1")),
                role(): eq(Role::Heading),
            })
        ))
    )
}

/// A state or property which is specific to roles does not stop the presentational role from
/// applying.
#[rstest]
#[case::aria_level(
    r#"<html><h1 role="presentation" aria-level="2">Text</h1></html>"#,
    "h1"
)]
#[case::aria_expanded(
    r#"<html><nav role="none" aria-expanded="true"><p>Text</p></nav></html>"#,
    "nav"
)]
fn does_not_expose_element_with_presentational_role_and_non_global_aria_attribute(
    #[case] html: &str,
    #[case] html_tag: &str,
) -> TestResult<()> {
    let tree_update = tree_of(html);

    verify_not_exposed(&tree_update, html_tag)
}

/// The elements which the element must contain inherit the presentational role.
#[rstest]
#[case::table(
    r#"<html><table role="presentation"><tr><td>Cell</td></tr></table></html>"#,
    "table"
)]
#[case::row(
    r#"<html><table role="presentation"><tr><td>Cell</td></tr></table></html>"#,
    "tr"
)]
#[case::cell(
    r#"<html><table role="presentation"><tr><td>Cell</td></tr></table></html>"#,
    "td"
)]
#[case::header_cell(
    r#"<html><table role="none"><tr><th>Header</th></tr></table></html>"#,
    "th"
)]
#[case::unordered_list(r#"<html><ul role="none"><li>Item</li></ul></html>"#, "ul")]
#[case::unordered_list_item(r#"<html><ul role="none"><li>Item</li></ul></html>"#, "li")]
#[case::ordered_list_item(r#"<html><ol role="presentation"><li>Item</li></ol></html>"#, "li")]
fn does_not_expose_required_owned_element_of_element_with_presentational_role(
    #[case] html: &str,
    #[case] html_tag: &str,
) -> TestResult<()> {
    let tree_update = tree_of(html);

    verify_not_exposed(&tree_update, html_tag)
}

/// Only the required owned elements inherit the role, not the other descendants.
#[rstest]
#[case::heading_in_cell(
    r#"<html><table role="presentation"><tr><td><h1>Text</h1></td></tr></table></html>"#,
    "h1",
    Role::Heading
)]
#[case::link_in_list_item(
    r##"<html><ul role="none"><li><a href="#target">Text</a></li></ul></html>"##,
    "a",
    Role::Link
)]
#[case::heading_in_navigation(
    r#"<html><nav role="none"><h1>Text</h1></nav></html>"#,
    "h1",
    Role::Heading
)]
fn keeps_role_of_descendant_which_is_not_a_required_owned_element(
    #[case] html: &str,
    #[case] html_tag: &str,
    #[case] role: Role,
) -> TestResult<()> {
    let tree_update = tree_of(html);

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                html_tag(): some(eq(html_tag)),
                role(): eq(role),
            })
        ))
    )
}

/// A required owned element with an explicit role which is not presentational keeps that role.
#[rstest]
#[case::list_item(
    r#"<html><ul role="none"><li role="button">Text</li></ul></html>"#,
    "li"
)]
#[case::list_item_of_ordered_list(
    r#"<html><ol role="presentation"><li role="button">Text</li></ol></html>"#,
    "li"
)]
fn keeps_explicit_role_of_required_owned_element_of_element_with_presentational_role(
    #[case] html: &str,
    #[case] html_tag: &str,
) -> TestResult<()> {
    let tree_update = tree_of(html);

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                html_tag(): some(eq(html_tag)),
                role(): eq(Role::Button),
            })
        ))
    )
}

/// Succeeds if the tree has no node for an element with the HTML tag, apart from one which has
/// the role [Role::GenericContainer].
fn verify_not_exposed(tree_update: &TreeUpdate, html_tag: &str) -> TestResult<()> {
    verify_that!(
        tree_update.nodes,
        not(contains((
            anything(),
            matches_pattern!(AccessKitNode {
                html_tag(): some(eq(html_tag)),
                role(): not(eq(Role::GenericContainer)),
            })
        )))
    )
}

/// The IDs of the nodes which can be reached from the root of the tree through the children.
fn reachable_node_ids(tree_update: &TreeUpdate) -> HashSet<NodeId> {
    let children: HashMap<NodeId, &[NodeId]> = tree_update
        .nodes
        .iter()
        .map(|(id, node)| (*id, node.children()))
        .collect();
    let mut reachable = HashSet::new();
    let mut pending = vec![tree_update.tree.as_ref().expect("the tree has a root").root];
    while let Some(id) = pending.pop() {
        if reachable.insert(id)
            && let Some(children_of_id) = children.get(&id)
        {
            pending.extend(children_of_id.iter().copied());
        }
    }
    reachable
}

fn tree_of(html: &str) -> TreeUpdate {
    let mut document = HtmlDocument::from_html(html, default_document_config());
    document.resolve(0.0);
    document.build_accessibility_tree()
}

fn default_document_config() -> DocumentConfig {
    DocumentConfig {
        viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
        html_parser_provider: Some(Arc::new(HtmlProvider) as _),
        ..Default::default()
    }
}
