//! `::first-letter` styles the first letter unit of a block's text.

use blitz_test_harness::Harness;

/// The text of each glyph run of `#test`'s inline layout, with its font size and
/// whether its brush is the node holding `#test`'s `::first-letter` style.
fn runs(html: &str) -> Vec<(String, f32, bool)> {
    let harness = Harness::from_html(html);
    let doc = harness.base();
    let id = doc.query_selector("#test").unwrap().unwrap();
    let node = doc.get_node(id).unwrap();
    let first_letter = node.first_letter();
    let text_layout = node
        .element_data()
        .unwrap()
        .inline_layout_data
        .as_ref()
        .unwrap();
    let mut runs = Vec::new();
    for line in text_layout.layout.lines() {
        for item in line.items() {
            if let parley::PositionedLayoutItem::GlyphRun(glyph_run) = item {
                let run = glyph_run.run();
                let text = text_layout.text[run.text_range()].to_string();
                let is_first_letter = Some(glyph_run.style().brush.id) == first_letter;
                runs.push((text, run.font_size(), is_first_letter));
            }
        }
    }
    runs
}

#[test]
fn first_letter_gets_its_style() {
    let runs = runs(
        "<style>p { font-size: 16px } p::first-letter { font-size: 48px }</style>\
         <p id='test'>Hello world</p>",
    );
    assert_eq!(runs[0], ("H".to_string(), 48.0, true), "{runs:?}");
    assert_eq!(runs[1], ("ello world".to_string(), 16.0, false), "{runs:?}");
}

#[test]
fn punctuation_around_the_first_letter_goes_with_it() {
    let runs = runs(
        "<style>p::first-letter { font-size: 48px }</style>\
         <p id='test'>  \u{201C}Hello,\u{201D} she said</p>",
    );
    assert_eq!(runs[0].0, "\u{201C}H", "{runs:?}");
    assert!(runs[0].2, "{runs:?}");
}

#[test]
fn first_letter_reaches_into_inline_children() {
    let runs = runs(
        "<style>p::first-letter { font-size: 48px }</style>\
         <p id='test'><b>Bold</b> start</p>",
    );
    assert_eq!(runs[0], ("B".to_string(), 48.0, true), "{runs:?}");
}

#[test]
fn an_image_before_the_text_means_no_first_letter() {
    let runs = runs(
        "<style>p::first-letter { font-size: 48px }</style>\
         <p id='test'><img style='width:10px;height:10px'>Text</p>",
    );
    assert!(runs.iter().all(|run| !run.2), "{runs:?}");
}

#[test]
fn without_a_rule_there_is_no_first_letter_node() {
    let harness = Harness::from_html("<p id='test'>Hello</p>");
    let doc = harness.base();
    let id = doc.query_selector("#test").unwrap().unwrap();
    assert_eq!(doc.get_node(id).unwrap().first_letter(), None);
}
