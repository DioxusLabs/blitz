//! Form controls use a UA-provided font family rather than inheriting the
//! page's (whose initial value is `serif`).

use blitz_test_harness::Harness;
use style::values::computed::font::{GenericFontFamily, SingleFontFamily};

fn first_family(harness: &Harness, selector: &str) -> SingleFontFamily {
    let id = harness.node(selector);
    let doc = harness.base();
    let styles = doc.get_node(id).unwrap().primary_styles().unwrap();
    styles.get_font().font_family.families.list[0].clone()
}

fn generic(family: GenericFontFamily) -> SingleFontFamily {
    SingleFontFamily::Generic(family)
}

#[test]
fn text_inputs_buttons_and_selects_use_the_system_ui_font() {
    let harness = Harness::from_html(
        "<body><input id=input><button id=button>b</button><select id=select></select></body>",
    );
    for selector in ["#input", "#button", "#select"] {
        assert_eq!(
            first_family(&harness, selector),
            generic(GenericFontFamily::SystemUi),
            "{selector}"
        );
    }
}

#[test]
fn textareas_use_a_monospace_font() {
    let harness = Harness::from_html("<body><textarea id=t></textarea></body>");
    assert_eq!(
        first_family(&harness, "#t"),
        generic(GenericFontFamily::Monospace)
    );
}

#[test]
fn other_elements_keep_the_initial_serif_font() {
    let harness = Harness::from_html("<body><p id=p>text</p></body>");
    assert_eq!(
        first_family(&harness, "#p"),
        generic(GenericFontFamily::Serif)
    );
}

#[test]
fn author_styles_override_the_form_control_font() {
    let harness = Harness::from_html(
        "<style>input { font-family: fantasy }</style><body><input id=input></body>",
    );
    assert_eq!(
        first_family(&harness, "#input"),
        generic(GenericFontFamily::Fantasy)
    );
}
