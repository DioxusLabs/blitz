//! A `font` element's `size` attribute maps to a font size keyword through the
//! HTML rules for parsing a legacy font size, and its `face` attribute sets
//! `font-family`.

use blitz_test_harness::Harness;
use style::properties::style_structs::Font;

/// The computed font of `<font {attrs}>` and of a span styled with `css`, both
/// inside a body whose font differs from every keyword size and from `initial`.
fn fonts(attrs: &str, css: &str) -> (Font, Font) {
    let harness = Harness::from_html(&format!(
        "<html><body style='font-size: 30px; font-family: cursive; --family: monospace'>\
         <font id=font {attrs}>x</font>\
         <span id=reference style='{css}'>x</span>\
         </body></html>"
    ));
    let font_of = |selector| {
        let id = harness.node(selector);
        let doc = harness.base();
        let styles = doc.get_node(id).unwrap().primary_styles().unwrap();
        styles.get_font().clone()
    };
    (font_of("#font"), font_of("#reference"))
}

fn assert_size_maps_to(size: &str, css_size: &str) {
    let (font, reference) = fonts(&format!("size='{size}'"), &format!("font-size: {css_size}"));
    assert_eq!(
        font.font_size.computed_size, reference.font_size.computed_size,
        "size={size:?}"
    );
}

#[test]
fn size_maps_to_a_keyword() {
    for (size, keyword) in [
        ("1", "x-small"),
        ("3", "medium"),
        ("7", "xxx-large"),
        ("+1", "large"),
        ("-1", "small"),
        // Values are clamped to 1..=7
        ("0", "x-small"),
        ("+5", "xxx-large"),
        ("-5", "x-small"),
        // Leading whitespace is skipped and anything after the digits ignored
        (" \t\n+2", "x-large"),
        ("5px", "x-large"),
        // Numbers too long for any integer type still clamp
        ("99999999999999999999", "xxx-large"),
        ("+99999999999999999999", "xxx-large"),
        ("-99999999999999999999", "x-small"),
    ] {
        assert_size_maps_to(size, keyword);
    }
}

#[test]
fn size_without_digits_is_ignored() {
    for size in ["", " ", "abc", "+", "-", "+-1", "+ 1"] {
        assert_size_maps_to(size, "inherit");
    }
}

#[test]
fn face_sets_the_font_family() {
    let (font, reference) = fonts("face='Georgia, serif'", "font-family: Georgia, serif");
    assert_eq!(font.font_family, reference.font_family);
}

/// Only a family list applies, as in Gecko and Blink: CSS-wide keywords and
/// `var()` are not family names.
#[test]
fn face_that_is_not_a_family_list_is_ignored() {
    for face in ["", "1px", "Georgia,", "initial", "var(--family)"] {
        let (font, reference) = fonts(&format!("face='{face}'"), "font-family: inherit");
        assert_eq!(font.font_family, reference.font_family, "face={face:?}");
    }
}
