//! `bgcolor`, `font color` and `body text` are read with the HTML "rules for
//! parsing a legacy colour value", which take more than CSS colors do.
//!
//! The engine only read `#rgb` and `#rrggbb`, and read `#rgb` wrong (`#fff`
//! as `#0f0f0f`), so the white tables of many emails came out near black.
//! `font color` and `body text` were not mapped at all.

use blitz_dom::CopyStyleGetters;
use blitz_test_harness::Harness;
use style::color::AbsoluteColor;

fn background_of(harness: &Harness, selector: &str) -> AbsoluteColor {
    let id = harness.node(selector);
    let doc = harness.base();
    let styles = doc.get_node(id).unwrap().primary_styles().unwrap();
    let current_color = styles.clone_color();
    (styles.get_background().background_color).resolve_to_absolute(&current_color)
}

fn color_of(harness: &Harness, selector: &str) -> AbsoluteColor {
    let id = harness.node(selector);
    let doc = harness.base();
    let styles = doc.get_node(id).unwrap().primary_styles().unwrap();
    styles.clone_color()
}

/// An element with `attributes`, and one styled with `css` to compare it to.
fn page(tag: &str, attributes: &str, css: &str) -> Harness {
    Harness::from_html(&format!(
        "<html><body>\
         <{tag} id=attr {attributes}>text</{tag}>\
         <{tag} id=css style='{css}'>text</{tag}>\
         </body></html>"
    ))
}

#[test]
fn bgcolor_takes_legacy_color_values() {
    for (value, css) in [
        ("#ff0000", "#ff0000"),
        ("#fff", "#ffffff"),
        ("#F00", "#ff0000"),
        ("red", "red"),
        ("LightGray", "lightgray"),
        ("ff0000", "#ff0000"),
        (" #ff0000 ", "#ff0000"),
        // Any other text is read digit by digit.
        ("chucknorris", "#c00000"),
    ] {
        let harness = page(
            "div",
            &format!("bgcolor='{value}'"),
            &format!("background-color: {css}"),
        );
        assert_eq!(
            background_of(&harness, "#attr"),
            background_of(&harness, "#css"),
            "bgcolor={value:?}"
        );
    }
}

#[test]
fn bgcolor_that_is_no_color_sets_no_background() {
    for value in ["", "transparent"] {
        let harness = page("div", &format!("bgcolor='{value}'"), "");
        assert_eq!(
            background_of(&harness, "#attr"),
            background_of(&harness, "#css"),
            "bgcolor={value:?}"
        );
    }
}

#[test]
fn font_color_and_body_text_set_the_text_color() {
    for (value, css) in [("#fff", "#ffffff"), ("red", "red"), ("336699", "#336699")] {
        let harness = page(
            "font",
            &format!("color='{value}'"),
            &format!("color: {css}"),
        );
        assert_eq!(
            color_of(&harness, "#attr"),
            color_of(&harness, "#css"),
            "color={value:?}"
        );
    }

    let harness = Harness::from_html(
        "<html><body text='#f00'><p id=p>text</p>\
         <p id=css style='color: #ff0000'>text</p></body></html>",
    );
    assert_eq!(color_of(&harness, "#p"), color_of(&harness, "#css"));
}

/// `color` is only mapped on `font`.
#[test]
fn color_on_other_elements_is_ignored() {
    let harness = page("p", "color=red", "");
    assert_eq!(color_of(&harness, "#attr"), color_of(&harness, "#css"));
}
