use blitz_dom::text::InlineText as _;
use blitz_test_harness::Harness;

fn layout_text(html: &str) -> String {
    let harness = Harness::from_html(html);
    let doc = harness.base();
    let id = doc.query_selector("#test").unwrap().unwrap();
    doc.get_node(id)
        .unwrap()
        .element_data()
        .unwrap()
        .inline_layout_data
        .as_ref()
        .unwrap()
        .text()
        .to_string()
}

#[test]
fn math_auto_is_inherited_and_can_be_overridden() {
    assert_eq!(
        layout_text(
            "<div id='test' style='text-transform:math-auto'>a<span>h</span>\
             <span style='display:contents'>α</span><span style='text-transform:none'>b</span>\
             <span style='text-transform:uppercase'>c</span></div>"
        ),
        "𝑎ℎ𝛼bC",
    );
}

#[test]
fn math_auto_counts_characters_before_whitespace_collapsing() {
    assert_eq!(
        layout_text(
            "<div id='test' style='text-transform:math-auto'><span> a </span>\
             <span>ab</span><span>a&#x301;</span><span>∂</span><span>∂∇</span></div>"
        ),
        "a aba\u{301}𝜕∂∇",
    );
}

#[test]
fn math_auto_transforms_adjacent_single_character_text_nodes() {
    assert_eq!(
        layout_text("<div id='test' style='text-transform:math-auto'>a<!--split-->b</div>"),
        "𝑎𝑏",
    );
}
