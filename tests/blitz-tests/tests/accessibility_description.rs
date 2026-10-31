use accesskit::{Node as AccessKitNode, Role};
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use rstest::rstest;
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
fn normalises_aria_description() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><div role="button" aria-description=" A  description "></div></html>"#,
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

#[test]
fn has_no_description_when_there_is_no_source() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><button>Content</button></html>"#,
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
                description(): none(),
            })
        ))
    )
}

#[test]
fn has_no_description_when_aria_describedby_targets_do_not_exist() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><div role="button" aria-describedby="nonexistent-element"></div></html>"#,
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
                description(): none(),
            })
        ))
    )
}

#[test]
fn has_no_description_when_aria_describedby_targets_have_no_text() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <div role="button" aria-describedby="empty"></div>
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
                description(): none(),
            })
        ))
    )
}

#[test]
fn does_not_use_aria_label_as_description() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><div role="button" aria-label="A label"></div></html>"#,
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
                label(): some(eq("A label")),
                description(): none(),
            })
        ))
    )
}

#[test]
fn does_not_use_text_of_aria_describedby_target_as_label() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <div role="button" aria-describedby="target"></div>
          <span id="target">Description</span>
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
                description(): some(eq("Description")),
            })
        ))
    )
}

#[test]
fn computes_label_and_description_independently() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <div role="button" aria-labelledby="label" aria-describedby="description"></div>
          <span id="label">Label</span>
          <span id="description">Description</span>
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
                label(): some(eq("Label")),
                description(): some(eq("Description")),
            })
        ))
    )
}

#[test]
fn uses_text_of_hidden_aria_describedby_target_without_referencing_it() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <div role="button" aria-describedby="hidden-element"></div>
          <span id="hidden-element" style="display: none">Hidden</span>
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
                description(): some(eq("Hidden")),
                *described_by(): is_empty(),
            })
        ))
    )
}

#[test]
fn uses_title_as_description_of_img_with_alt() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><img alt="Alt text" title="Title text"></html>"#,
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
                description(): some(eq("Title text")),
            })
        ))
    )
}

#[test]
fn uses_title_as_description_of_input_named_by_label_element() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html>
          <label for="arbitrary-input">Name</label>
          <input id="arbitrary-input" title="Hint">
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
                label(): some(eq("Name")),
                description(): some(eq("Hint")),
            })
        ))
    )
}

#[test]
fn does_not_use_title_as_description_when_it_provides_the_name() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><button title="Tip text"></button></html>"#,
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
                description(): none(),
            })
        ))
    )
}

#[test]
fn uses_title_as_description_of_element_whose_role_prohibits_naming() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><div title="Tip">text</div></html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                html_tag(): some(eq("div")),
                label(): none(),
                description(): some(eq("Tip")),
            })
        ))
    )
}

#[test]
fn uses_title_as_description_of_img_with_empty_alt() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><img alt="" title="Title text"></html>"#,
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
                description(): some(eq("Title text")),
            })
        ))
    )
}

#[test]
fn does_not_use_title_as_description_of_img_without_alt() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><img title="Title text"></html>"#,
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
                description(): none(),
            })
        ))
    )
}

#[test]
fn uses_title_as_description_when_aria_descrpition_is_blank() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><div aria-description="  " title="Title text"></div></html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                html_tag(): some(eq("div")),
                description(): some(eq("Title text")),
            })
        ))
    )
}

#[test]
fn ignores_blank_title_as_description() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><button title="   ">Save</button></html>"#,
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
                description(): none(),
            })
        ))
    )
}

#[rstest]
#[case::uses_text_of_aria_describedby_targets_in_attribute_order(
    r#"<html>
          <div role="button" aria-describedby="second first"></div>
          <span id="first">  First </span>
          <span id="second">Second
            label</span>
        </html>"#,
    "Second label First"
)]
#[case::uses_aria_label_of_aria_describedby_target(
    r#"<html>
          <div role="button" aria-describedby="target"></div>
          <span id="target" aria-label="From aria-label">From content</span>
        </html>"#,
    "From aria-label"
)]
#[case::aria_describedby_takes_precedence_over_aria_description(
    r#"<html>
          <div role="button" aria-description="Ignored" aria-describedby="target"></div>
          <span id="target">Referenced</span>
        </html>"#,
    "Referenced"
)]
#[case::falls_back_to_aria_description_when_aria_describedby_targets_have_no_text(
    r#"<html>
          <div role="button" aria-description="Fallback" aria-describedby="empty"></div>
          <span id="empty"></span>
        </html>"#,
    "Fallback"
)]
#[case::ignores_invalid_idrefs_among_valid_aria_describedby_idrefs(
    r#"<html>
          <div role="button" aria-describedby="nonexistent-element first"></div>
          <span id="first">First</span>
        </html>"#,
    "First"
)]
#[case::does_not_follow_aria_labelledby_of_aria_describedby_target(
    r#"<html>
          <div role="button" aria-describedby="first"></div>
          <span id="first" aria-labelledby="second">First</span>
          <span id="second">Second</span>
        </html>"#,
    "First"
)]
#[case::does_not_follow_aria_describedby_of_aria_describedby_target(
    r#"<html>
          <div role="button" aria-describedby="first"></div>
          <span id="first" aria-describedby="second">First</span>
          <span id="second">Second</span>
        </html>"#,
    "First"
)]
#[case::ignores_aria_description_of_aria_describedby_target(
    r#"<html>
          <div role="button" aria-describedby="target"></div>
          <span id="target" aria-description="Ignored">Text</span>
        </html>"#,
    "Text"
)]
#[case::uses_text_of_aria_hidden_aria_describedby_target(
    r#"<html>
          <div role="button" aria-describedby="hidden-element"></div>
          <span id="hidden-element" aria-hidden="true">Save</span>
        </html>"#,
    "Save"
)]
#[case::uses_hidden_aria_describedby_targets_for_description_without_referencing_them(
    r#"<html>
          <div role="button" aria-describedby="hidden-element hidden-descendant"></div>
          <span id="hidden-element" style="display: none">Save</span>
          <div style="visibility: hidden"><span id="hidden-descendant">file</span></div>
        </html>"#,
    "Save file"
)]
#[case::includes_self_reference_and_nested_text_in_aria_describedby(
    r#"<html>
          <div role="button" id="self" aria-describedby="self label">Delete</div>
          <span id="label">file<b>name</b></span>
        </html>"#,
    "Delete filename"
)]
#[case::uses_text_of_label_element_referenced_by_aria_describedby(
    r#"<html>
          <div role="button" aria-describedby="target"></div>
          <label id="target">Save <span>the file</span></label>
        </html>"#,
    "Save the file"
)]
#[case::includes_img_alt_in_aria_describedby_target_text(
    r#"<html>
          <div role="button" aria-describedby="nested-img direct-img"></div>
          <span id="nested-img">Save <img alt="logo"></span>
          <img id="direct-img" alt="now" aria-label="">
        </html>"#,
    "Save logo now"
)]
#[case::prefers_img_aria_label_over_alt_in_aria_describedby_target(
    r#"<html>
          <div role="button" aria-describedby="target"></div>
          <img id="target" alt="Alt text" aria-label="Aria label">
        </html>"#,
    "Aria label"
)]
#[case::concatenates_elements_with_space_between_them_when_markup_does_not_include_whitespace(
    r#"<html><div role="button" aria-describedby="target"></div><div id="target"><div>Node</div><div>content</div></div></html>"#,
    "Node content"
)]
#[case::separates_text_from_following_block_element_within_inline_parent(
    r#"<html><div role="button" aria-describedby="target"></div><span id="target">Save<div>file</div></span></html>"#,
    "Save file"
)]
#[case::does_not_separate_adjacent_inline_content_in_block_parent(
    r#"<html><div role="button" aria-describedby="target"></div><div id="target">Sa<b>ve</b></div></html>"#,
    "Save"
)]
#[case::does_not_separate_text_around_comment(
    r#"<html><div role="button" aria-describedby="target"></div><div id="target">Sa<!-- arbitrary comment -->ve</div></html>"#,
    "Save"
)]
#[case::does_not_separate_text_around_display_none_element(
    r#"<html><div role="button" aria-describedby="target"></div><div id="target">Sa<span style="display: none">arbitrary</span>ve</div></html>"#,
    "Save"
)]
#[case::does_not_separate_text_around_display_contents_inline_element(
    r#"<html><div role="button" aria-describedby="target"></div><div id="target">Sa<span style="display: contents">v</span>e</div></html>"#,
    "Save"
)]
#[case::separates_table_cells(
    r#"<html><div role="button" aria-describedby="target"></div><div id="target"><table><tr><td>First</td><td>second</td></tr></table></div></html>"#,
    "First second"
)]
#[case::separates_elements_with_display_table_cell(
    r#"<html><div role="button" aria-describedby="target"></div><div id="target"><span style="display: table-cell">First</span><span style="display: table-cell">second</span></div></html>"#,
    "First second"
)]
#[case::separates_floated_element_from_surrounding_text(
    r#"<html><div role="button" aria-describedby="target"></div><div id="target"><span style="float: left">First</span>second</div></html>"#,
    "First second"
)]
#[case::separates_absolutely_positioned_element_from_surrounding_text(
    r#"<html><div role="button" aria-describedby="target"></div><div id="target"><span style="position: absolute">First</span>second</div></html>"#,
    "First second"
)]
#[case::separates_img_alt_from_adjacent_text(
    r#"<html><div role="button" aria-describedby="target"></div><div id="target">Save<img alt="the">file</div></html>"#,
    "Save the file"
)]
#[case::separates_embedded_input_value_from_adjacent_text(
    r#"<html><div role="button" aria-describedby="target"></div><div id="target">Flash<input value="5">times</div></html>"#,
    "Flash 5 times"
)]
#[case::separates_inline_block_element_from_adjacent_text(
    r#"<html><div role="button" aria-describedby="target"></div><div id="target">Save<span style="display: inline-block">the</span>file</div></html>"#,
    "Save the file"
)]
#[case::does_not_separate_text_around_inline_element_containing_only_aria_hidden_block(
    r#"<html><div role="button" aria-describedby="target"></div><div id="target">Sa<span><div aria-hidden="true">arbitrary</div></span>ve</div></html>"#,
    "Save"
)]
#[case::separates_text_around_line_break(
    r#"<html><div role="button" aria-describedby="target"></div><div id="target">First<br>second</div></html>"#,
    "First second"
)]
#[case::does_not_separate_text_around_hidden_line_break(
    r#"<html><div role="button" aria-describedby="target"></div><div id="target">First<br style="display: none;">second</div></html>"#,
    "Firstsecond"
)]
#[case::separates_block_before_pseudo_element_from_content(
    r#"<html>
          <style>.arbitrary::before { content: "First"; display: block; }</style>
          <div role="button" aria-describedby="target"></div>
          <div id="target"><div class="arbitrary">second</div></div>
        </html>"#,
    "First second"
)]
#[case::separates_block_after_pseudo_element_from_content(
    r#"<html>
          <style>.arbitrary::after { content: "second"; display: block; }</style>
          <div role="button" aria-describedby="target"></div>
          <div id="target"><div class="arbitrary">First</div></div>
        </html>"#,
    "First second"
)]
#[case::separates_inline_block_pseudo_element_from_content(
    r#"<html>
          <style>.arbitrary::before { content: "First"; display: inline-block; }</style>
          <div role="button" aria-describedby="target"></div>
          <div id="target"><span class="arbitrary">second</span></div>
        </html>"#,
    "First second"
)]
#[case::joins_inline_pseudo_elements_to_content_without_space(
    r#"<html>
          <style>
            .arbitrary::before { content: "Sa"; }
            .arbitrary::after { content: "e"; }
          </style>
          <div role="button" aria-describedby="target"></div>
          <div id="target"><span class="arbitrary">v</span></div>
        </html>"#,
    "Save"
)]
#[case::excludes_hidden_descendants_of_visible_aria_describedby_target(
    r#"<html>
          <div role="button" aria-describedby="target"></div>
          <span id="target">Save
            <span style="display: none">secret</span>
            <span aria-hidden="true">hidden</span>
            <span style="visibility: hidden">invisible</span>
            file</span>
        </html>"#,
    "Save file"
)]
#[case::includes_hidden_descendants_of_hidden_aria_describedby_target(
    r#"<html>
          <div role="button" aria-describedby="target"></div>
          <div id="target" style="visibility: hidden">Save
            <span style="display: none">file</span></div>
        </html>"#,
    "Save file"
)]
#[case::uses_aria_label_of_descendants_of_aria_describedby_target(
    r#"<html>
          <div role="button" aria-describedby="target"></div>
          <span id="target">Save <span aria-label="the file">ignored</span> now
            <b aria-label="  ">blank aria-label is ignored</b></span>
        </html>"#,
    "Save the file now blank aria-label is ignored"
)]
#[case::uses_value_of_embedded_text_inputs(
    r#"<html>
          <div role="button" aria-describedby="target"></div>
          <span id="target">Flash <input value="5"> times and
            <textarea>notes</textarea> then <input type="password" value="secret"> done</span>
        </html>"#,
    "Flash 5 times and notes then done"
)]
#[case::uses_value_of_directly_referenced_text_input(
    r#"<html>
          <div role="button" aria-describedby="referenced"></div>
          <input id="referenced" value="Entered" aria-label="Ignored">
        </html>"#,
    "Entered"
)]
#[case::uses_value_of_embedded_range_controls(
    r#"<html>
          <div role="button" aria-describedby="target"></div>
          <span id="target">
            <div role="slider" aria-valuenow="3" aria-valuetext="three"></div>
            <div role="spinbutton" aria-valuenow="4" aria-valuemin="0"></div>
            <input type="range" value="7">
          </span>
        </html>"#,
    "three 4 7"
)]
#[case::uses_chosen_option_of_embedded_select(
    r#"<html>
          <div role="button" aria-describedby="target"></div>
          <span id="target">
            Send
            <select><option>One</option><option selected>Two</option></select>
            and
            <select><option>Three</option><option>Four</option></select>
          </span>
        </html>"#,
    "Send Two and Three"
)]
#[case::separates_chosen_options_of_embedded_multiple_select(
    r#"<html>
          <div role="button" aria-describedby="target"></div>
          <span id="target">Pick
            <select multiple>
              <option selected>One</option>
              <option>Two</option>
              <option selected>Three</option>
            </select>
            now</span>
        </html>"#,
    "Pick One Three now"
)]
#[case::separates_selected_options_of_embedded_listbox(
    r#"<html>
          <div role="button" aria-describedby="target"></div>
          <span id="target">Pick
            <div role="listbox">
              <div role="option" aria-selected="true">One</div>
              <div role="option" aria-selected="false">Two</div>
              <div role="option" aria-selected="true">Three</div>
            </div>
            now</span>
        </html>"#,
    "Pick One Three now"
)]
#[case::includes_generated_content_of_aria_describedby_target_and_descendants(
    r#"<html>
          <head><style>
            #target::before { content: "Before-"; }
            #target::after { content: "-after"; }
            #inner::before { content: "["; }
            #inner::after { content: "]"; }
          </style></head>
          <body>
            <div role="button" aria-describedby="target"></div>
            <span id="target">text <span id="inner">inner</span></span>
          </body>
        </html>"#,
    "Before-text [inner]-after"
)]
#[case::uses_title_as_description_when_name_comes_from_content(
    r#"<html><button title="Tip text">Save</button></html>"#,
    "Tip text"
)]
#[case::uses_title_as_description_when_name_comes_from_aria_label(
    r#"<html><button title="Tip text" aria-label="Label">x</button></html>"#,
    "Tip text"
)]
#[case::aria_describedby_takes_precedence_over_title(
    r#"<html>
          <div role="button" title="Ignored" aria-describedby="target"></div>
          <span id="target">Referenced</span>
        </html>"#,
    "Referenced"
)]
#[case::aria_description_takes_precedence_over_title(
    r#"<html><button title="Ignored" aria-description="Description">Save</button></html>"#,
    "Description"
)]
#[case::collapses_whitespace_in_title_used_as_description(
    r#"<html><button title="  Tip   text ">Save</button></html>"#,
    "Tip text"
)]
fn button_has_description(
    #[case] html: &str,
    #[case] expected_description: &str,
) -> TestResult<()> {
    let mut document = HtmlDocument::from_html(html, default_document_config());
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::Button),
                description(): some(eq(expected_description)),
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
