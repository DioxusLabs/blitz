use blitz_dom::{BaseDocument, node::TextLayout};
use blitz_test_harness::Harness;

fn inline<'a>(doc: &'a BaseDocument, selector: &str) -> &'a TextLayout {
    let id = doc.query_selector(selector).unwrap().unwrap();
    doc.get_node(id)
        .unwrap()
        .element_data()
        .unwrap()
        .inline_layout_data
        .as_ref()
        .unwrap()
}

#[test]
fn pre_line_collapses_across_spans_and_preserves_breaks() {
    let harness = Harness::from_html(
        "<div id='test' style='white-space:pre-line'>a \t <span> b \t\n c </span> d<br> e  f</div>",
    );
    let doc = harness.base();
    let text = inline(&doc, "#test");
    assert_eq!(text.text, "a b\nc d\ne f");
    assert_eq!(text.layout.len(), 3);
    assert!(text.layout.full_width() > 0.);
}

#[test]
fn preserved_spans_and_br_restore_parent_collapsing() {
    let harness = Harness::from_html(
        "<div id='test'><span style='white-space:pre'>x  y<br>z  w</span>  q</div>",
    );
    let doc = harness.base();
    let text = inline(&doc, "#test");
    assert_eq!(text.text, "x  y\nz  w q");
    assert_eq!(text.layout.len(), 2);
}

#[test]
fn display_contents_preserves_its_whitespace_mode() {
    let harness = Harness::from_html(
        "<div id='test'>a<span style='display:contents;white-space:pre'>b  c</span> d  e</div>",
    );
    let doc = harness.base();
    assert_eq!(inline(&doc, "#test").text, "ab  c d e");
}

#[test]
fn display_contents_keeps_wrapping_independent_from_collapsing() {
    for (mode, lines) in [("pre", 1), ("nowrap", 1), ("pre-wrap", 2), ("normal", 2)] {
        let harness = Harness::from_html(&format!(
            "<div id='test' style='width:1px'><span style='display:contents;white-space:{mode}'>one two</span></div>",
        ));
        let doc = harness.base();
        assert_eq!(inline(&doc, "#test").layout.len(), lines, "{mode}");
    }
}

#[test]
fn out_of_flow_placeholder_retains_its_text_offset() {
    let harness = Harness::from_html(
        "<div id='test'>un<span style='position:absolute'>box</span>broken</div>",
    );
    let doc = harness.base();
    let text = inline(&doc, "#test");
    assert_eq!(text.text, "unbroken");
    assert_eq!(text.layout.inline_boxes().len(), 1);
    assert_eq!(text.layout.inline_boxes()[0].index, 2);
    assert_eq!(text.layout.len(), 1);
}

#[test]
fn break_spaces_wraps_preserved_spaces_across_spans_and_br() {
    for display in ["inline", "contents"] {
        let harness = Harness::from_html(&format!(
            "<div id='test' style='width:1px;white-space:break-spaces'>a <span style='display:{display}'>  </span>b<br>  c</div>",
        ));
        let doc = harness.base();
        let text = inline(&doc, "#test");
        assert_eq!(text.text, "a   b\n  c");
        let lines: Vec<_> = text
            .layout
            .lines()
            .map(|line| &text.text[line.text_range()])
            .collect();
        assert_eq!(lines, ["a ", " ", " ", "b\n", " ", " ", "c"]);
        for line in text.layout.lines() {
            assert_eq!(line.metrics().hanging_advance, 0.);
        }
    }
}

#[test]
fn display_contents_break_spaces_restores_parent_collapsing() {
    let harness = Harness::from_html(
        "<div id='test'>a <span style='display:contents;white-space:break-spaces'>  b<br>  c</span>  d</div>",
    );
    let doc = harness.base();
    let text = inline(&doc, "#test");
    assert_eq!(text.text, "a   b\n  c d");
    assert_eq!(text.layout.len(), 2);
}

#[test]
fn break_spaces_includes_whitespace_in_intrinsic_widths_and_alignment() {
    let harness = Harness::from_html(
        "<style>div{white-space:break-spaces;text-align:right;width:200px}</style>\
         <div id='one'>a </div><div id='two'>a  </div>\
         <div id='nowrap' style='text-wrap-mode:nowrap'>a  </div>",
    );
    let doc = harness.base();
    let one = &inline(&doc, "#one").layout;
    let two = &inline(&doc, "#two").layout;
    let nowrap = &inline(&doc, "#nowrap").layout;
    let one_widths = one.calculate_content_widths();
    let two_widths = two.calculate_content_widths();
    assert!(two_widths.max > one_widths.max);
    assert!((two_widths.min - one_widths.max).abs() < 0.01);
    assert!((nowrap.calculate_content_widths().min - two_widths.max).abs() < 0.01);
    let line = two.get(0).unwrap();
    assert!((line.metrics().offset + line.metrics().advance - 200.).abs() < 0.01);
    assert_eq!(line.metrics().hanging_advance, 0.);
}
