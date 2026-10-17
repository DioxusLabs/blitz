use accesskit::{Node as AccessKitNode, Role};
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;
use test_that::prelude::*;

#[test]
fn includes_aria_label_as_label() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><div role="button" aria-label="A node"></div></html>"#,
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
                label(): some(eq("A node")),
            })
        ))
    )
}

#[test]
fn includes_aria_labelledby_as_labelled_by_for_one_node() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <div role="button" aria-labelledby="arbitrary-element"></div>
          <label id="arbitrary-element">Label</label>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    let (label_node_id, _) = tree_update
        .nodes
        .iter()
        .find(|(_, node)| node.role() == Role::Label)
        .expect("Label must be present in accessibility tree");
    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::Button),
                *labelled_by(): container_eq(vec![*label_node_id]),
            })
        ))
    )
}

#[test]
fn includes_aria_labelledby_as_labelled_by_for_two_nodes() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <div role="button" aria-labelledby="arbitrary-element another-element"></div>
          <label id="another-element">Another label</label>
          <nav id="arbitrary-element">Label</nav>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    // This awkward construction is due to having to fetch the node IDs for the label
    // elements in the same order they appear in the aria-labelledby attribute. The
    // order is also important to the assertion.
    let mut expected_node_ids = Vec::new();
    expected_node_ids.push(
        tree_update
            .nodes
            .iter()
            .find(|(_, node)| node.role() == Role::Navigation)
            .expect("Expected to find label arbitrary-element in the tree")
            .0,
    );
    expected_node_ids.push(
        tree_update
            .nodes
            .iter()
            .find(|(_, node)| node.role() == Role::Label)
            .expect("Expected to find label another-element in the tree")
            .0,
    );
    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::Button),
                *labelled_by(): container_eq(expected_node_ids),
            })
        ))
    )
}

#[test]
fn ignores_labelledby_element_which_does_not_correspond_to_element_in_the_dom() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><div role="button" aria-labelledby="nonexistent-element"></div></html>"#,
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
                *labelled_by(): is_empty(),
            })
        ))
    )
}

#[test]
fn uses_text_of_aria_labelledby_targets_as_label() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <div role="button" aria-labelledby="second first"></div>
          <span id="first">  First </span>
          <span id="second">Second
            label</span>
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
                label(): some(eq("Second label First")),
            })
        ))
    )
}

#[test]
fn aria_labelledby_takes_precedence_over_aria_label_and_content() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <button aria-label="Ignored" aria-labelledby="label">Click</button>
          <span id="label">Save</span>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    let (span_node_id, _) = tree_update
        .nodes
        .iter()
        .find(|(_, node)| node.html_tag() == Some("span"))
        .expect("Span must be present in accessibility tree");
    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::Button),
                label(): some(eq("Save")),
                *labelled_by(): container_eq(vec![*span_node_id]),
            })
        ))
    )
}

#[test]
fn includes_self_reference_and_nested_text_in_aria_labelledby() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <div role="button" id="self" aria-labelledby="self label">Delete</div>
          <span id="label">file<b>name</b></span>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    let (button_node_id, _) = tree_update
        .nodes
        .iter()
        .find(|(_, node)| node.role() == Role::Button)
        .expect("Button must be present in accessibility tree");
    let (span_node_id, _) = tree_update
        .nodes
        .iter()
        .find(|(_, node)| node.html_tag() == Some("span"))
        .expect("Span must be present in accessibility tree");
    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::Button),
                label(): some(eq("Delete filename")),
                *labelled_by(): container_eq(vec![*button_node_id, *span_node_id]),
            })
        ))
    )
}

#[test]
fn uses_aria_label_of_aria_labelledby_target() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <div role="button" aria-labelledby="label"></div>
          <span id="label" aria-label="From aria-label">From content</span>
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
                label(): some(eq("From aria-label")),
            })
        ))
    )
}

#[test]
fn uses_hidden_aria_labelledby_targets_for_label_without_referencing_them() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <div role="button" aria-labelledby="hidden-element hidden-descendant"></div>
          <span id="hidden-element" style="display: none">Save</span>
          <div style="visibility: hidden"><span id="hidden-descendant">file</span></div>
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
                label(): some(eq("Save file")),
                *labelled_by(): is_empty(),
            })
        ))
    )
}

#[test]
fn falls_back_to_aria_label_when_aria_labelledby_targets_have_no_text() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <div role="button" aria-label="Fallback" aria-labelledby="empty"></div>
          <span id="empty"></span>
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
                label(): some(eq("Fallback")),
            })
        ))
    )
}

#[test]
fn uses_img_alt_text_as_accessible_name() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <img alt="Image label">
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
                role(): eq(Role::Image),
                label(): some(eq("Image label")),
            })
        ))
    )
}

#[test]
fn populates_labelled_by_from_label_element() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <button id="arbitrary-button"></button>
          <label for="arbitrary-button">Button label</label>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    let (label_node_id, _) = tree_update
        .nodes
        .iter()
        .find(|(_, node)| node.role() == Role::Label)
        .expect("Label must be present in accessibility tree");
    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::Button),
                *labelled_by(): container_eq([*label_node_id]),
            })
        ))
    )
}

#[test]
fn uses_text_of_label_element_as_label_of_target() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <button id="arbitrary-button"></button>
          <label for="arbitrary-button">  Button   label </label>
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
                label(): some(eq("Button label")),
            })
        ))
    )
}

#[test]
fn populates_labelled_by_when_label_element_precedes_target() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <label for="arbitrary-button">Button label</label>
          <button id="arbitrary-button"></button>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    let (label_node_id, _) = tree_update
        .nodes
        .iter()
        .find(|(_, node)| node.role() == Role::Label)
        .expect("Label must be present in accessibility tree");
    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::Button),
                *labelled_by(): container_eq([*label_node_id]),
                label(): some(eq("Button label")),
            })
        ))
    )
}

#[test]
fn includes_all_label_elements_referencing_the_same_target_in_document_order() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <label for="arbitrary-button">First</label>
          <button id="arbitrary-button"></button>
          <label for="arbitrary-button">Second</label>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    let mut label_node_ids: Vec<_> = tree_update
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == Role::Label)
        .map(|(id, _)| *id)
        .collect();
    label_node_ids.sort_by_key(|id| id.0);
    verify_that!(label_node_ids, len(eq(2)))?;
    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::Button),
                *labelled_by(): container_eq(label_node_ids),
                label(): some(eq("First Second")),
            })
        ))
    )
}

#[test]
fn labels_each_target_with_its_own_label_element() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <button id="first-button"></button>
          <label for="first-button">First label</label>
          <button id="second-button"></button>
          <label for="second-button">Second label</label>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        all!(
            contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::Button),
                    label(): some(eq("First label")),
                })
            )),
            contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::Button),
                    label(): some(eq("Second label")),
                })
            ))
        )
    )
}

#[test]
fn uses_text_of_nested_elements_in_label_element() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <button id="arbitrary-button"></button>
          <label for="arbitrary-button">Save <span>the file</span> now</label>
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
                label(): some(eq("Save the file now")),
            })
        ))
    )
}

#[test]
fn label_element_takes_precedence_over_content_of_target() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <button id="arbitrary-button">Content</button>
          <label for="arbitrary-button">Label</label>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    let (label_node_id, _) = tree_update
        .nodes
        .iter()
        .find(|(_, node)| node.role() == Role::Label)
        .expect("Label must be present in accessibility tree");
    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::Button),
                label(): some(eq("Label")),
                *labelled_by(): container_eq([*label_node_id]),
            })
        ))
    )
}

#[test]
fn aria_labelledby_takes_precedence_over_label_element() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <button id="arbitrary-button" aria-labelledby="referenced"></button>
          <label for="arbitrary-button">Label element</label>
          <span id="referenced">Referenced</span>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    let (span_node_id, _) = tree_update
        .nodes
        .iter()
        .find(|(_, node)| node.html_tag() == Some("span"))
        .expect("Span must be present in accessibility tree");
    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::Button),
                label(): some(eq("Referenced")),
                *labelled_by(): container_eq([*span_node_id]),
            })
        ))
    )
}

#[test]
fn aria_label_takes_precedence_over_label_element() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <button id="arbitrary-button" aria-label="Aria label"></button>
          <label for="arbitrary-button">Label element</label>
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
                label(): some(eq("Aria label")),
                *labelled_by(): is_empty(),
            })
        ))
    )
}

#[test]
fn ignores_label_element_whose_for_does_not_match_any_element() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <button id="arbitrary-button"></button>
          <label for="nonexistent-element">Label</label>
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
                *labelled_by(): is_empty(),
                label(): none(),
            })
        ))
    )
}

#[test]
fn ignores_label_element_without_for_attribute() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <button id="arbitrary-button"></button>
          <label>Label</label>
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
                *labelled_by(): is_empty(),
                label(): none(),
            })
        ))
    )
}

#[test]
fn ignores_label_element_which_is_hidden() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <button id="arbitrary-button"></button>
          <label for="arbitrary-button" style="display: none;">Label</label>
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
                *labelled_by(): is_empty(),
                label(): none(),
            })
        ))
    )
}

#[test]
fn ignores_label_element_whose_target_is_hidden() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <button id="arbitrary-button" style="display: none;"></button>
          <label for="arbitrary-button">Label</label>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        all!(
            contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::Label),
                })
            )),
            not(contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::Button),
                })
            )))
        )
    )
}

#[test]
fn associates_label_element_with_first_element_having_the_referenced_id() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <button id="duplicate"></button>
          <nav id="duplicate" role="checkbox"></nav>
          <label for="duplicate">Label</label>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        all!(
            contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::Button),
                    label(): some(eq("Label")),
                })
            )),
            contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::CheckBox),
                    *labelled_by(): is_empty(),
                })
            ))
        )
    )
}

#[test]
fn blank_aria_label_does_not_take_precedence_over_label_element() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <button id="arbitrary-button" aria-label="   "></button>
          <label for="arbitrary-button">Label element</label>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    let (label_node_id, _) = tree_update
        .nodes
        .iter()
        .find(|(_, node)| node.role() == Role::Label)
        .expect("Label must be present in accessibility tree");
    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::Button),
                label(): some(eq("Label element")),
                *labelled_by(): container_eq([*label_node_id]),
            })
        ))
    )
}

#[test]
fn aria_label_takes_precedence_over_img_alt() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><img alt="Alt text" aria-label="Aria label"></html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::Image),
                label(): some(eq("Aria label")),
            })
        ))
    )
}

#[test]
fn aria_labelledby_takes_precedence_over_img_alt() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <img alt="Alt text" aria-labelledby="referenced">
          <span id="referenced">Referenced</span>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    let (span_node_id, _) = tree_update
        .nodes
        .iter()
        .find(|(_, node)| node.html_tag() == Some("span"))
        .expect("Span must be present in accessibility tree");
    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::Image),
                label(): some(eq("Referenced")),
                *labelled_by(): container_eq([*span_node_id]),
            })
        ))
    )
}

#[test]
fn img_without_alt_has_no_label() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(r#"<html><img></html>"#, default_document_config());
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::Image),
                label(): none(),
                *labelled_by(): is_empty(),
            })
        ))
    )
}

#[test]
fn populates_label_of_text_input_textarea_and_select_from_label_element() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <label for="text-input">Input label</label>
          <input id="text-input">
          <label for="text-area">Textarea label</label>
          <textarea id="text-area"></textarea>
          <label for="select-box">Select label</label>
          <select id="select-box"><option>One</option></select>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        all!(
            contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::TextInput),
                    label(): some(eq("Input label")),
                    *labelled_by(): len(eq(1)),
                })
            )),
            contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::MultilineTextInput),
                    label(): some(eq("Textarea label")),
                    *labelled_by(): len(eq(1)),
                })
            )),
            contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::ComboBox),
                    label(): some(eq("Select label")),
                    *labelled_by(): len(eq(1)),
                })
            ))
        )
    )
}

#[test]
fn ignores_invalid_idrefs_among_valid_aria_labelledby_idrefs() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <div role="button" aria-labelledby="nonexistent-element first"></div>
          <span id="first">First</span>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    let (span_node_id, _) = tree_update
        .nodes
        .iter()
        .find(|(_, node)| node.html_tag() == Some("span"))
        .expect("Span must be present in accessibility tree");
    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::Button),
                label(): some(eq("First")),
                *labelled_by(): container_eq([*span_node_id]),
            })
        ))
    )
}

#[test]
fn does_not_follow_aria_labelledby_of_aria_labelledby_target() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <div role="button" aria-labelledby="first"></div>
          <span id="first" aria-labelledby="second">First</span>
          <span id="second">Second</span>
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
                label(): some(eq("First")),
            })
        ))
    )
}

#[test]
fn uses_text_of_aria_hidden_aria_labelledby_target_as_label() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <div role="button" aria-labelledby="hidden-element"></div>
          <span id="hidden-element" aria-hidden="true">Save</span>
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
                label(): some(eq("Save")),
            })
        ))
    )
}

#[test]
fn ignores_blank_aria_label() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <div role="button" aria-label=""></div>
          <div role="checkbox" aria-label="   "></div>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        all!(
            contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::Button),
                    label(): none(),
                })
            )),
            contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::CheckBox),
                    label(): none(),
                })
            ))
        )
    )
}

#[test]
fn falls_back_to_img_alt_when_aria_label_is_blank() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><img alt="Alt text" aria-label="  "></html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::Image),
                label(): some(eq("Alt text")),
            })
        ))
    )
}

#[test]
fn ignores_empty_and_blank_img_alt() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><img alt=""><img alt="   "></html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    let imgs: Vec<_> = tree_update
        .nodes
        .iter()
        .filter(|(_, node)| node.html_tag() == Some("img"))
        .collect();
    verify_that!(imgs, len(eq(2)))?;
    verify_that!(
        imgs.iter()
            .map(|(_, node)| node.label())
            .collect::<Vec<_>>(),
        each(none())
    )
}

#[test]
fn uses_figcaption_as_label_of_img_without_alt() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <figure>
            <img>
            <figcaption>Caption <b>text</b></figcaption>
          </figure>
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
                role(): eq(Role::Image),
                label(): some(eq("Caption text")),
            })
        ))
    )
}

#[test]
fn uses_figcaption_as_label_of_img_nested_in_figure_content() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <figure>
            <div><a href="arbitrary"><img></a></div>
            <figcaption>Caption</figcaption>
          </figure>
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
                role(): eq(Role::Image),
                label(): some(eq("Caption")),
            })
        ))
    )
}

#[test]
fn img_alt_takes_precedence_over_figcaption() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <figure>
            <img alt="Alt text">
            <figcaption>Caption</figcaption>
          </figure>
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
                role(): eq(Role::Image),
                label(): some(eq("Alt text")),
            })
        ))
    )
}

#[test]
fn does_not_use_figcaption_for_img_with_empty_alt() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <figure>
            <img alt="">
            <figcaption>Caption</figcaption>
          </figure>
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
                html_tag(): some(eq("img")),
                label(): none(),
            })
        ))
    )
}

#[test]
fn does_not_use_figcaption_for_img_with_title() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <figure>
            <img title="Title text">
            <figcaption>Caption</figcaption>
          </figure>
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
                html_tag(): some(eq("img")),
                label(): none(),
            })
        ))
    )
}

#[test]
fn does_not_use_figcaption_for_img_in_figure_with_other_content() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <figure>
            <img>
            <p>Other content</p>
            <figcaption>Caption</figcaption>
          </figure>
          <figure>
            <img>
            <img>
            <figcaption>Another caption</figcaption>
          </figure>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    let imgs: Vec<_> = tree_update
        .nodes
        .iter()
        .filter(|(_, node)| node.html_tag() == Some("img"))
        .collect();
    verify_that!(imgs, len(eq(3)))?;
    verify_that!(
        imgs.iter()
            .map(|(_, node)| node.label())
            .collect::<Vec<_>>(),
        each(none())
    )
}

#[test]
fn does_not_use_figcaption_for_img_outside_figure_with_figcaption() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <img>
          <figure><figcaption>Caption</figcaption></figure>
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
                role(): eq(Role::Image),
                label(): none(),
            })
        ))
    )
}

#[test]
fn includes_img_alt_in_label_element_text() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <button id="arbitrary-button"></button>
          <label for="arbitrary-button">Save <img alt="the file"> now</label>
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
                label(): some(eq("Save the file now")),
            })
        ))
    )
}

#[test]
fn includes_img_alt_in_aria_labelledby_target_text() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <div role="button" aria-labelledby="nested-img direct-img"></div>
          <span id="nested-img">Save <img alt="logo"></span>
          <img id="direct-img" alt="now" aria-label="">
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
                label(): some(eq("Save logo now")),
            })
        ))
    )
}

#[test]
fn prefers_img_aria_label_over_alt_in_label_element_text() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <button id="arbitrary-button"></button>
          <label for="arbitrary-button"><img alt="Alt text" aria-label="Aria label"></label>
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
                label(): some(eq("Aria label")),
            })
        ))
    )
}

#[test]
fn ignores_label_element_whose_target_is_not_labelable() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <div id="div-target" role="button"></div>
          <label for="div-target">Div label</label>
          <nav id="nav-target" role="checkbox"></nav>
          <label for="nav-target">Nav label</label>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        all!(
            contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::Button),
                    label(): none(),
                    *labelled_by(): is_empty(),
                })
            )),
            contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::CheckBox),
                    label(): none(),
                    *labelled_by(): is_empty(),
                })
            ))
        )
    )
}

#[test]
fn ignores_label_element_when_first_element_with_the_referenced_id_is_not_labelable()
-> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <div id="duplicate"></div>
          <button id="duplicate"></button>
          <label for="duplicate">Label</label>
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
                label(): none(),
                *labelled_by(): is_empty(),
            })
        ))
    )
}

#[test]
fn populates_label_of_meter_output_progress_and_checkbox_from_label_element() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <label for="meter-target">Meter label</label>
          <meter id="meter-target"></meter>
          <label for="output-target">Output label</label>
          <output id="output-target"></output>
          <label for="progress-target">Progress label</label>
          <progress id="progress-target"></progress>
          <label for="checkbox-target">Checkbox label</label>
          <input id="checkbox-target" type="checkbox">
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        all!(
            contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::Meter),
                    label(): some(eq("Meter label")),
                })
            )),
            contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::Status),
                    label(): some(eq("Output label")),
                })
            )),
            contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::ProgressIndicator),
                    label(): some(eq("Progress label")),
                })
            )),
            contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::CheckBox),
                    label(): some(eq("Checkbox label")),
                })
            ))
        )
    )
}

#[test]
fn populates_label_from_label_element_wrapping_the_target() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><label>Name <input></label></html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    let (label_node_id, _) = tree_update
        .nodes
        .iter()
        .find(|(_, node)| node.role() == Role::Label)
        .expect("Label must be present in accessibility tree");
    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::TextInput),
                label(): some(eq("Name")),
                *labelled_by(): container_eq([*label_node_id]),
            })
        ))
    )
}

#[test]
fn excludes_wrapped_target_content_from_label_text() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <label>Notes <textarea>Draft</textarea> here</label>
          <label>Save <button>Go</button> now</label>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        all!(
            contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::MultilineTextInput),
                    label(): some(eq("Notes here")),
                })
            )),
            contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::Button),
                    label(): some(eq("Save now")),
                })
            ))
        )
    )
}

#[test]
fn wrapping_label_element_labels_only_its_first_labelable_descendant() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <label>
            <span>Label</span>
            <input>
            <textarea></textarea>
          </label>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        all!(
            contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::TextInput),
                    label(): some(eq("Label")),
                })
            )),
            contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::MultilineTextInput),
                    label(): none(),
                    *labelled_by(): is_empty(),
                })
            ))
        )
    )
}

#[test]
fn for_attribute_takes_precedence_over_wrapped_target() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <label for="referenced">Label <input></label>
          <button id="referenced"></button>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        all!(
            contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::Button),
                    label(): some(eq("Label")),
                })
            )),
            contains((
                anything(),
                matches_pattern!(AccessKitNode {
                    role(): eq(Role::TextInput),
                    label(): none(),
                    *labelled_by(): is_empty(),
                })
            ))
        )
    )
}

#[test]
fn does_not_label_wrapped_target_when_for_attribute_references_non_labelable_element()
-> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <label for="not-labelable">Label <input></label>
          <nav id="not-labelable"></nav>
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
                role(): eq(Role::TextInput),
                label(): none(),
                *labelled_by(): is_empty(),
            })
        ))
    )
}

#[test]
fn combines_wrapping_and_for_label_elements_in_document_order() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <label>Outer <input id="target"></label>
          <label for="target">Second</label>
        </html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    let mut label_node_ids: Vec<_> = tree_update
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == Role::Label)
        .map(|(id, _)| *id)
        .collect();
    label_node_ids.sort_by_key(|id| id.0);
    verify_that!(label_node_ids, len(eq(2)))?;
    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::TextInput),
                label(): some(eq("Outer Second")),
                *labelled_by(): container_eq(label_node_ids),
            })
        ))
    )
}

#[test]
fn aria_label_of_wrapped_target_takes_precedence_over_wrapping_label_element() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><label>Label <input aria-label="Aria label"></label></html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::TextInput),
                label(): some(eq("Aria label")),
                *labelled_by(): is_empty(),
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
