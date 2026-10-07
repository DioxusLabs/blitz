use blitz_dom::{Document, DocumentConfig};
use blitz_vibey_script::ScriptDocument;

fn select(html: &str, script: &str) -> Vec<String> {
    let mut doc = ScriptDocument::from_html(html, DocumentConfig::default());
    doc.eval(script);
    assert_eq!(doc.take_js_errors(), Vec::<String>::new());
    doc.take_messages()
}

#[test]
fn selection_identity_and_empty_state() {
    assert_eq!(
        select(
            "<div>x</div>",
            r#"
        const s = window.getSelection();
        __blitz_send_message(String(s === getSelection() && s === document.getSelection()));
        __blitz_send_message(String(s));
        s.removeAllRanges();
        __blitz_send_message(s.toString());
    "#
        ),
        ["true", "", ""]
    );
}

#[test]
fn selects_math_auto_transformed_character() {
    assert_eq!(
        select(
            "<span id='test' style='text-transform:math-auto'>h</span>",
            r#"
        const text = test.firstChild;
        getSelection().setBaseAndExtent(text, 0, text, 1);
        __blitz_send_message(String(getSelection()));
        __blitz_send_message(text.data);
    "#
        ),
        ["ℎ", "h"]
    );
}

#[test]
fn selection_uses_utf16_offsets_in_both_directions() {
    assert_eq!(
        select(
            "<div id='test'>a😀éb</div>",
            r#"
        const text = test.firstChild, s = getSelection();
        s.setBaseAndExtent(text, 1, text, 4);
        __blitz_send_message(String(s));
        s.setBaseAndExtent(text, 4, text, 1);
        __blitz_send_message(String(s));
        s.setBaseAndExtent(text, 2, text, 2);
        __blitz_send_message(String(s));
    "#
        ),
        ["😀é", "😀é", ""]
    );
}

#[test]
fn maps_multiple_text_nodes_and_element_child_offsets() {
    assert_eq!(
        select(
            "<div id='test'>hello <b id='bold'>world</b>!</div>",
            r#"
        const s = getSelection();
        s.setBaseAndExtent(test.firstChild, 3, bold.firstChild, 3);
        __blitz_send_message(String(s));
        s.setBaseAndExtent(test, 1, test, 2);
        __blitz_send_message(String(s));
        s.setBaseAndExtent(test, 0, test, 3);
        __blitz_send_message(String(s));
    "#
        ),
        ["lo wor", "world", "hello world!"]
    );
}

#[test]
fn maps_collapsed_whitespace_and_case_expansions() {
    assert_eq!(
        select(
            "<div id='test' style='text-transform:uppercase'>  a  ß  b </div>",
            r#"
        const s = getSelection(), text = test.firstChild;
        s.setBaseAndExtent(text, 5, text, 6);
        __blitz_send_message(String(s));
        s.setBaseAndExtent(text, 0, text, text.data.length);
        __blitz_send_message(String(s));
    "#
        ),
        ["SS", "A SS B"]
    );
}

#[test]
fn selection_is_recomputed_after_style_changes() {
    assert_eq!(
        select(
            "<div id='test'>h</div>",
            r#"
        const s = getSelection(), text = test.firstChild;
        s.setBaseAndExtent(text, 0, text, 1);
        __blitz_send_message(String(s));
        test.style.textTransform = 'math-auto';
        __blitz_send_message(String(s));
        test.remove();
        __blitz_send_message(String(s));
    "#
        ),
        ["h", "ℎ", ""]
    );
}

#[test]
fn invalid_offsets_throw_without_changing_selection() {
    assert_eq!(
        select(
            "<div id='test'>abc</div>",
            r#"
        const s = getSelection(), text = test.firstChild;
        s.setBaseAndExtent(text, 0, text, 1);
        try { s.setBaseAndExtent(text, 0, text, 4); }
        catch (error) { __blitz_send_message(error.name); }
        const detached = document.createTextNode('x');
        s.setBaseAndExtent(detached, 0, detached, 1);
        __blitz_send_message(String(s));
        s.empty();
        __blitz_send_message(String(s));
    "#
        ),
        ["IndexSizeError", "a", ""]
    );
}

#[test]
fn selects_across_inline_roots_and_anonymous_blocks() {
    assert_eq!(
        select(
            "<div id='test'>before<p id='middle'>middle</p>after</div>",
            r#"
        const s = getSelection();
        s.setBaseAndExtent(test.firstChild, 2, test.lastChild, 3);
        __blitz_send_message(String(s));
        s.setBaseAndExtent(test.lastChild, 3, test.firstChild, 2);
        __blitz_send_message(String(s));
    "#
        ),
        ["fore middle aft", "fore middle aft"]
    );
}

#[test]
fn hidden_content_and_comments_do_not_shift_offsets() {
    assert_eq!(
        select(
            "<div id='test'>a<span style='display:none'>invisible</span><!--comment-->bc</div>",
            r#"
        const s = getSelection();
        s.setBaseAndExtent(test.lastChild, 0, test.lastChild, 1);
        __blitz_send_message(String(s));
        s.setBaseAndExtent(test.childNodes[2], 3, test.lastChild, 1);
        __blitz_send_message(String(s));
    "#
        ),
        ["b", "b"]
    );
}

#[test]
fn preserves_whitespace_and_forced_line_breaks() {
    assert_eq!(
        select(
            "<div id='test' style='white-space:pre'>a  b<br>c</div>",
            r#"
        const s = getSelection();
        s.setBaseAndExtent(test, 0, test, 3);
        __blitz_send_message(String(s));
    "#
        ),
        ["a  b\nc"]
    );
}

#[test]
fn element_boundaries_map_across_block_children() {
    assert_eq!(
        select(
            "<div id='test'><p>abc</p><p>def</p></div>",
            r#"
        const s = getSelection();
        s.setBaseAndExtent(test, 0, test, 2);
        __blitz_send_message(String(s));
        s.setBaseAndExtent(test, 1, test, 2);
        __blitz_send_message(String(s));
    "#
        ),
        ["abc def", "def"]
    );
}

#[test]
fn preserves_pre_line_segment_breaks() {
    assert_eq!(
        select(
            "<div id='test' style='white-space:pre-line'>a  \n  b</div>",
            r#"
        const s = getSelection(), text = test.firstChild;
        s.setBaseAndExtent(text, 0, text, text.data.length);
        __blitz_send_message(String(s));
        s.setBaseAndExtent(text, 6, text, 7);
        __blitz_send_message(String(s));
    "#
        ),
        ["a\nb", "b"]
    );
}

#[test]
fn native_selection_changes_replace_script_selection() {
    let mut doc = ScriptDocument::from_html("<div id='test'>abc</div>", DocumentConfig::default());
    doc.eval(
        "const s = getSelection(); s.setBaseAndExtent(test.firstChild, 1, test.firstChild, 3);",
    );
    {
        let mut inner = doc.inner_mut();
        let root = inner.query_selector("#test").unwrap().unwrap();
        inner.set_text_selection(root, 0, root, 1);
    }
    doc.eval("__blitz_send_message(String(s));");
    doc.inner_mut().clear_text_selection();
    doc.eval("__blitz_send_message(String(s));");
    assert_eq!(doc.take_js_errors(), Vec::<String>::new());
    assert_eq!(doc.take_messages(), ["a", ""]);
}

#[test]
fn removed_combining_marks_do_not_consume_the_next_character() {
    assert_eq!(
        select(
            "<span id='test' lang='el' style='text-transform:uppercase'>Ϊ́Ρ</span>",
            r#"
        const s = getSelection(), text = test.firstChild;
        s.setBaseAndExtent(text, 0, text, 3);
        __blitz_send_message(String(s));
        s.setBaseAndExtent(text, 3, text, 4);
        __blitz_send_message(String(s));
    "#
        ),
        ["Ϊ", "Ρ"]
    );
}

#[test]
fn retains_dom_endpoints_when_anonymous_inline_roots_are_rebuilt() {
    let mut doc =
        ScriptDocument::from_html("<div id='test'>h<p>x</p>y</div>", DocumentConfig::default());
    doc.eval("const s = getSelection(); s.setBaseAndExtent(test.firstChild, 0, test.firstChild, 1); test.style.textTransform = 'math-auto';");
    doc.inner_mut().resolve(0.0);
    doc.eval("__blitz_send_message(String(s));");
    assert_eq!(doc.take_js_errors(), Vec::<String>::new());
    assert_eq!(doc.take_messages(), ["ℎ"]);
}
