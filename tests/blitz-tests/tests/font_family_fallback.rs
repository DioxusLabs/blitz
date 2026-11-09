//! When none of the families in a `font-family` list is available, browsers use
//! the default font (the initial `serif`) rather than the system fallback for
//! the script, both for text and for font-relative units such as `ch`.

use blitz_test_harness::Harness;

const TEXT: &str = "Hamburgefonstiv";

fn page() -> Harness {
    let row = |id: &str, family: &str| {
        format!(
            "<span id={id} style='display: inline-block; font-family: {family}'>{TEXT}</span>\
             <div id={id}-ch style='font-family: {family}; width: 10ch; height: 1px'></div>"
        )
    };
    Harness::from_html(&format!(
        "<html><body style='font-size: 20px'>{}{}{}</body></html>",
        row("missing", "\"No Such Family\""),
        row("serif", "serif"),
        row("sans", "sans-serif"),
    ))
}

#[test]
fn unavailable_families_fall_back_to_the_default_font() {
    let harness = page();
    let width = |selector: &str| harness.layout_rect(selector).width;
    // Without distinct serif and sans-serif fonts there is nothing to tell apart
    if width("#serif") == width("#sans") {
        return;
    }
    assert_eq!(width("#missing"), width("#serif"));
    assert_eq!(width("#missing-ch"), width("#serif-ch"));
}
