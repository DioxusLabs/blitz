use accesskit::{Node as AccessKitNode, Role};
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use rstest::rstest;
use std::sync::Arc;
use test_that::prelude::*;

#[test]
fn sets_the_placeholder_from_the_native_property() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><input placeholder="Arbitrary placeholder"></input></html>"#,
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
                placeholder(): some(eq("Arbitrary placeholder")),
            })
        ))
    )
}

#[test]
fn sets_the_placeholder_from_the_aria_property() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><input aria-placeholder="Arbitrary placeholder"></input></html>"#,
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
                placeholder(): some(eq("Arbitrary placeholder")),
            })
        ))
    )
}

#[test]
fn native_placeholder_overrides_aria_placeholder() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><input placeholder="Correct placeholder" aria-placeholder="Incorrect placeholder"></html>"#,
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
                placeholder(): some(eq("Correct placeholder")),
            })
        ))
    )
}

#[test]
fn normalizes_the_placeholder_text() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><input aria-placeholder="Arbitrary  placeholder"></input></html>"#,
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
                placeholder(): some(eq("Arbitrary placeholder")),
            })
        ))
    )
}

#[rstest]
#[case::empty_native_placeholder(r#"<html><input placeholder=""></html>"#, "input")]
#[case::blank_native_placeholder(r#"<html><input placeholder="   "></html>"#, "input")]
#[case::native_placeholder_of_only_line_breaks_and_tabs(
    r#"<html><input placeholder="&#10;&#9; &#13;"></html>"#,
    "input"
)]
#[case::empty_aria_placeholder(r#"<html><input aria-placeholder=""></html>"#, "input")]
#[case::blank_aria_placeholder(r#"<html><input aria-placeholder="  "></html>"#, "input")]
#[case::blank_native_and_aria_placeholders(
    r#"<html><input placeholder="  " aria-placeholder="&#10;"></html>"#,
    "input"
)]
#[case::empty_placeholder_of_textarea(
    r#"<html><textarea placeholder=""></textarea></html>"#,
    "textarea"
)]
#[case::blank_placeholder_of_textarea(
    r#"<html><textarea placeholder="&#10; "></textarea></html>"#,
    "textarea"
)]
fn ignores_blank_placeholder(#[case] html: &str, #[case] html_tag: &str) -> TestResult<()> {
    let mut document = HtmlDocument::from_html(html, default_document_config());
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                html_tag(): some(eq(html_tag)),
                placeholder(): none(),
            })
        ))
    )
}

#[rstest]
#[case::empty_native_placeholder(
    r#"<html><input placeholder="" aria-placeholder="Fallback placeholder"></html>"#
)]
#[case::blank_native_placeholder(
    r#"<html><input placeholder="   " aria-placeholder="Fallback placeholder"></html>"#
)]
#[case::native_placeholder_of_only_line_breaks(
    r#"<html><input placeholder="&#10;&#10;" aria-placeholder="Fallback placeholder"></html>"#
)]
fn falls_back_to_aria_placeholder_when_native_placeholder_is_blank(
    #[case] html: &str,
) -> TestResult<()> {
    let mut document = HtmlDocument::from_html(html, default_document_config());
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::TextInput),
                placeholder(): some(eq("Fallback placeholder")),
            })
        ))
    )
}

#[rstest]
#[case::button(
    r#"<html><button placeholder="Arbitrary placeholder">Label</button></html>"#,
    "button"
)]
#[case::select(
    r#"<html><select placeholder="Arbitrary placeholder"><option>One</option></select></html>"#,
    "select"
)]
#[case::div(
    r#"<html><div placeholder="Arbitrary placeholder"></div></html>"#,
    "div"
)]
#[case::span(
    r#"<html><span placeholder="Arbitrary placeholder"></span></html>"#,
    "span"
)]
#[case::paragraph(r#"<html><p placeholder="Arbitrary placeholder"></p></html>"#, "p")]
#[case::heading(
    r#"<html><h1 placeholder="Arbitrary placeholder">Title</h1></html>"#,
    "h1"
)]
#[case::section(
    r#"<html><section placeholder="Arbitrary placeholder"></section></html>"#,
    "section"
)]
#[case::link(
    r##"<html><a href="#target" placeholder="Arbitrary placeholder">Link</a></html>"##,
    "a"
)]
#[case::image(
    r#"<html><img alt="Alt" placeholder="Arbitrary placeholder"></html>"#,
    "img"
)]
#[case::label(
    r#"<html><label placeholder="Arbitrary placeholder">Label</label></html>"#,
    "label"
)]
fn ignores_native_placeholder_of_element_which_does_not_support_it(
    #[case] html: &str,
    #[case] html_tag: &str,
) -> TestResult<()> {
    let mut document = HtmlDocument::from_html(html, default_document_config());
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                html_tag(): some(eq(html_tag)),
                placeholder(): none(),
            })
        ))
    )
}

#[rstest]
#[case::input(r#"<html><input placeholder="Arbitrary placeholder"></html>"#, "input")]
#[case::textarea(
    r#"<html><textarea placeholder="Arbitrary placeholder"></textarea></html>"#,
    "textarea"
)]
fn sets_the_native_placeholder_of_element_which_supports_it(
    #[case] html: &str,
    #[case] html_tag: &str,
) -> TestResult<()> {
    let mut document = HtmlDocument::from_html(html, default_document_config());
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                html_tag(): some(eq(html_tag)),
                placeholder(): some(eq("Arbitrary placeholder")),
            })
        ))
    )
}

#[rstest]
#[case::textbox_on_div(
    r#"<html><div role="textbox" aria-placeholder="Arbitrary placeholder"></div></html>"#,
    Role::TextInput
)]
#[case::textbox_on_span(
    r#"<html><span role="textbox" aria-placeholder="Arbitrary placeholder"></span></html>"#,
    Role::TextInput
)]
#[case::textbox_on_input(
    r#"<html><input role="textbox" aria-placeholder="Arbitrary placeholder"></html>"#,
    Role::TextInput
)]
#[case::searchbox_on_div(
    r#"<html><div role="searchbox" aria-placeholder="Arbitrary placeholder"></div></html>"#,
    Role::SearchInput
)]
#[case::input_with_implicit_textbox_role(
    r#"<html><input aria-placeholder="Arbitrary placeholder"></html>"#,
    Role::TextInput
)]
#[case::input_with_implicit_searchbox_role(
    r#"<html><input type="search" aria-placeholder="Arbitrary placeholder"></html>"#,
    Role::SearchInput
)]
#[case::textarea(
    r#"<html><textarea aria-placeholder="Arbitrary placeholder"></textarea></html>"#,
    Role::MultilineTextInput
)]
fn sets_aria_placeholder_of_element_with_role_which_supports_it(
    #[case] html: &str,
    #[case] role: Role,
) -> TestResult<()> {
    let mut document = HtmlDocument::from_html(html, default_document_config());
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(role),
                placeholder(): some(eq("Arbitrary placeholder")),
            })
        ))
    )
}

#[rstest]
#[case::no_role(
    r#"<html><div aria-placeholder="Arbitrary placeholder"></div></html>"#,
    "div"
)]
#[case::button_role(
    r#"<html><div role="button" aria-placeholder="Arbitrary placeholder"></div></html>"#,
    "div"
)]
#[case::checkbox_role(
    r#"<html><div role="checkbox" aria-placeholder="Arbitrary placeholder"></div></html>"#,
    "div"
)]
#[case::link_role(
    r#"<html><div role="link" aria-placeholder="Arbitrary placeholder"></div></html>"#,
    "div"
)]
#[case::heading_role(
    r#"<html><div role="heading" aria-placeholder="Arbitrary placeholder"></div></html>"#,
    "div"
)]
#[case::combobox_role(
    r#"<html><div role="combobox" aria-placeholder="Arbitrary placeholder"></div></html>"#,
    "div"
)]
#[case::spinbutton_role(
    r#"<html><div role="spinbutton" aria-placeholder="Arbitrary placeholder"></div></html>"#,
    "div"
)]
#[case::button_element(
    r#"<html><button aria-placeholder="Arbitrary placeholder">Label</button></html>"#,
    "button"
)]
#[case::checkbox_input(
    r#"<html><input type="checkbox" aria-placeholder="Arbitrary placeholder"></html>"#,
    "input"
)]
#[case::input_with_overridden_role(
    r#"<html><input role="button" aria-placeholder="Arbitrary placeholder"></html>"#,
    "input"
)]
fn ignores_aria_placeholder_of_element_with_role_which_does_not_support_it(
    #[case] html: &str,
    #[case] html_tag: &str,
) -> TestResult<()> {
    let mut document = HtmlDocument::from_html(html, default_document_config());
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                html_tag(): some(eq(html_tag)),
                placeholder(): none(),
            })
        ))
    )
}

#[rstest]
#[case::text("text")]
#[case::search("search")]
#[case::tel("tel")]
#[case::url("url")]
#[case::email("email")]
#[case::password("password")]
#[case::number("number")]
#[case::unknown_type("arbitrary-unknown-type")]
fn sets_native_placeholder_of_input_of_type_which_supports_it(
    #[case] input_type: &str,
) -> TestResult<()> {
    let tree_update = tree_of(&format!(
        r#"<html><input type="{input_type}" placeholder="Arbitrary placeholder"></html>"#
    ));

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                html_tag(): some(eq("input")),
                placeholder(): some(eq("Arbitrary placeholder")),
            })
        ))
    )
}

#[rstest]
#[case::checkbox("checkbox")]
#[case::radio("radio")]
#[case::range("range")]
#[case::color("color")]
#[case::date("date")]
#[case::datetime_local("datetime-local")]
#[case::time("time")]
#[case::month("month")]
#[case::week("week")]
#[case::file("file")]
#[case::image("image")]
#[case::button("button")]
#[case::submit("submit")]
#[case::reset("reset")]
fn ignores_native_placeholder_of_input_of_type_which_does_not_support_it(
    #[case] input_type: &str,
) -> TestResult<()> {
    let tree_update = tree_of(&format!(
        r#"<html><input type="{input_type}" placeholder="Arbitrary placeholder"></html>"#
    ));

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                html_tag(): some(eq("input")),
                placeholder(): none(),
            })
        ))
    )
}

#[rstest]
#[case::text("text")]
#[case::search("search")]
#[case::tel("tel")]
#[case::url("url")]
#[case::email("email")]
#[case::password("password")]
fn sets_aria_placeholder_of_input_of_type_with_text_box_role(
    #[case] input_type: &str,
) -> TestResult<()> {
    let tree_update = tree_of(&format!(
        r#"<html><input type="{input_type}" aria-placeholder="Arbitrary placeholder"></html>"#
    ));

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                html_tag(): some(eq("input")),
                placeholder(): some(eq("Arbitrary placeholder")),
            })
        ))
    )
}

#[rstest]
#[case::textbox_on_div(
    r#"<html><div role="textbox" placeholder="Arbitrary placeholder"></div></html>"#,
    "div"
)]
#[case::searchbox_on_span(
    r#"<html><span role="searchbox" placeholder="Arbitrary placeholder"></span></html>"#,
    "span"
)]
#[case::textbox_on_checkbox_input(
    r#"<html><input type="checkbox" role="textbox" placeholder="Arbitrary placeholder"></html>"#,
    "input"
)]
fn ignores_native_placeholder_of_element_which_only_has_text_box_role(
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
                placeholder(): none(),
            })
        ))
    )
}

#[test]
fn uses_aria_placeholder_when_native_placeholder_is_not_supported_by_element() -> TestResult<()> {
    let tree_update = tree_of(
        r#"<html><div role="textbox" placeholder="Native placeholder" aria-placeholder="Aria placeholder"></div></html>"#,
    );

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                html_tag(): some(eq("div")),
                placeholder(): some(eq("Aria placeholder")),
            })
        ))
    )
}

#[test]
fn sets_native_placeholder_of_input_whatever_its_role() -> TestResult<()> {
    let tree_update =
        tree_of(r#"<html><input role="button" placeholder="Arbitrary placeholder"></html>"#);

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(Role::Button),
                placeholder(): some(eq("Arbitrary placeholder")),
            })
        ))
    )
}

#[rstest]
#[case::input_with_spaces(
    r#"<html><input placeholder="  Arbitrary   placeholder  "></html>"#,
    "input",
    "Arbitrary placeholder"
)]
#[case::input_with_tab(
    r#"<html><input placeholder="Arbitrary&#9;placeholder"></html>"#,
    "input",
    "Arbitrary placeholder"
)]
#[case::textarea_with_line_break(
    r#"<html><textarea placeholder="First line&#10;Second line"></textarea></html>"#,
    "textarea",
    "First line Second line"
)]
#[case::textarea_with_carriage_return_and_line_feed(
    r#"<html><textarea placeholder="First line&#13;&#10;Second line"></textarea></html>"#,
    "textarea",
    "First line Second line"
)]
fn normalizes_the_native_placeholder_text(
    #[case] html: &str,
    #[case] html_tag: &str,
    #[case] expected: &str,
) -> TestResult<()> {
    let tree_update = tree_of(html);

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                html_tag(): some(eq(html_tag)),
                placeholder(): some(eq(expected)),
            })
        ))
    )
}

#[rstest]
#[case::input_with_value(
    r#"<html><input value="Arbitrary value" placeholder="Arbitrary placeholder"></html>"#,
    "input"
)]
#[case::textarea_with_content(
    r#"<html><textarea placeholder="Arbitrary placeholder">Arbitrary value</textarea></html>"#,
    "textarea"
)]
fn sets_the_placeholder_of_control_which_has_a_value(
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
                placeholder(): some(eq("Arbitrary placeholder")),
            })
        ))
    )
}

#[test]
fn sets_the_placeholder_of_control_inside_aria_hidden_element() -> TestResult<()> {
    let tree_update = tree_of(
        r#"<html><div aria-hidden="true"><input placeholder="Arbitrary placeholder"></div></html>"#,
    );

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                html_tag(): some(eq("input")),
                placeholder(): some(eq("Arbitrary placeholder")),
            })
        ))
    )
}

#[test]
fn sets_the_placeholder_of_aria_hidden_control() -> TestResult<()> {
    let tree_update =
        tree_of(r#"<html><input aria-hidden="true" placeholder="Arbitrary placeholder"></html>"#);

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                html_tag(): some(eq("input")),
                is_hidden(): eq(true),
                placeholder(): some(eq("Arbitrary placeholder")),
            })
        ))
    )
}

#[rstest]
#[case::checkbox("CHECKBOX")]
#[case::radio("Radio")]
#[case::range("RANGE")]
#[case::color("Color")]
#[case::date("DATE")]
#[case::datetime_local("DateTime-Local")]
#[case::time("TIME")]
#[case::month("Month")]
#[case::week("WEEK")]
#[case::file("File")]
#[case::image("IMAGE")]
#[case::button("BUTTON")]
#[case::submit("Submit")]
#[case::reset("RESET")]
fn ignores_native_placeholder_of_input_of_type_which_does_not_support_it_regardless_of_case(
    #[case] input_type: &str,
) -> TestResult<()> {
    let tree_update = tree_of(&format!(
        r#"<html><input type="{input_type}" placeholder="Arbitrary placeholder"></html>"#
    ));

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                html_tag(): some(eq("input")),
                placeholder(): none(),
            })
        ))
    )
}

#[rstest]
#[case::text("TEXT")]
#[case::search("Search")]
#[case::tel("TEL")]
#[case::url("Url")]
#[case::email("EMAIL")]
#[case::password("Password")]
#[case::number("NUMBER")]
fn sets_native_placeholder_of_input_of_type_which_supports_it_regardless_of_case(
    #[case] input_type: &str,
) -> TestResult<()> {
    let tree_update = tree_of(&format!(
        r#"<html><input type="{input_type}" placeholder="Arbitrary placeholder"></html>"#
    ));

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                html_tag(): some(eq("input")),
                placeholder(): some(eq("Arbitrary placeholder")),
            })
        ))
    )
}

#[rstest]
#[case::text("TEXT")]
#[case::search("Search")]
#[case::tel("TEL")]
#[case::url("Url")]
#[case::email("EMAIL")]
#[case::password("Password")]
fn sets_aria_placeholder_of_input_of_type_with_text_box_role_regardless_of_case(
    #[case] input_type: &str,
) -> TestResult<()> {
    let tree_update = tree_of(&format!(
        r#"<html><input type="{input_type}" aria-placeholder="Arbitrary placeholder"></html>"#
    ));

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                html_tag(): some(eq("input")),
                placeholder(): some(eq("Arbitrary placeholder")),
            })
        ))
    )
}

#[rstest]
#[case::file("file")]
#[case::image("image")]
#[case::month("month")]
#[case::week("week")]
#[case::checkbox_in_upper_case("CHECKBOX")]
#[case::file_in_upper_case("FILE")]
fn ignores_aria_placeholder_of_input_of_type_without_text_box_role(
    #[case] input_type: &str,
) -> TestResult<()> {
    let tree_update = tree_of(&format!(
        r#"<html><input type="{input_type}" aria-placeholder="Arbitrary placeholder"></html>"#
    ));

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                html_tag(): some(eq("input")),
                placeholder(): none(),
            })
        ))
    )
}

#[rstest]
#[case::list_starting_with_searchbox(
    r#"<html><div role="searchbox textbox" aria-placeholder="Arbitrary placeholder"></div></html>"#,
    Role::SearchInput
)]
#[case::list_starting_with_unknown_role(
    r#"<html><div role="arbitrary-unknown-role textbox" aria-placeholder="Arbitrary placeholder"></div></html>"#,
    Role::TextInput
)]
#[case::list_separated_by_tab(
    r#"<html><div role="arbitrary-unknown-role&#9;textbox" aria-placeholder="Arbitrary placeholder"></div></html>"#,
    Role::TextInput
)]
#[case::upper_case_textbox(
    r#"<html><div role="TEXTBOX" aria-placeholder="Arbitrary placeholder"></div></html>"#,
    Role::TextInput
)]
#[case::mixed_case_searchbox(
    r#"<html><div role="Searchbox" aria-placeholder="Arbitrary placeholder"></div></html>"#,
    Role::SearchInput
)]
#[case::surrounding_whitespace(
    r#"<html><div role="  textbox  " aria-placeholder="Arbitrary placeholder"></div></html>"#,
    Role::TextInput
)]
fn sets_aria_placeholder_of_element_whose_role_attribute_resolves_to_text_box_role(
    #[case] html: &str,
    #[case] role: Role,
) -> TestResult<()> {
    let tree_update = tree_of(html);

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(role),
                placeholder(): some(eq("Arbitrary placeholder")),
            })
        ))
    )
}

#[rstest]
#[case::list_starting_with_button(
    r#"<html><div role="button textbox" aria-placeholder="Arbitrary placeholder"></div></html>"#,
    Role::Button
)]
#[case::list_starting_with_unknown_role(
    r#"<html><div role="arbitrary-unknown-role button" aria-placeholder="Arbitrary placeholder"></div></html>"#,
    Role::Button
)]
#[case::unknown_role_only(
    r#"<html><div role="arbitrary-unknown-role" aria-placeholder="Arbitrary placeholder"></div></html>"#,
    Role::GenericContainer
)]
fn ignores_aria_placeholder_of_element_whose_role_attribute_resolves_to_other_role(
    #[case] html: &str,
    #[case] role: Role,
) -> TestResult<()> {
    let tree_update = tree_of(html);

    verify_that!(
        tree_update.nodes,
        contains((
            anything(),
            matches_pattern!(AccessKitNode {
                role(): eq(role),
                placeholder(): none(),
            })
        ))
    )
}

fn tree_of(html: &str) -> accesskit::TreeUpdate {
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
