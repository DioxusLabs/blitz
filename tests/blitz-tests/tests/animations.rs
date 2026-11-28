use blitz_dom::CopyStyleGetters;
use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;

/// Regression test for https://github.com/DioxusLabs/blitz/issues/407
///
/// `resolve_stylist` used to panic with "invalid key" when a CSS-animated node
/// was removed between two `resolve` calls. Verify the second resolve is safe.
#[test]
fn resolve_does_not_panic_after_removing_animated_node() {
    let html = r#"
        <style>
          @keyframes pulse { 0% { opacity: 1; } 100% { opacity: 0.5; } }
          .pulse { animation: pulse 2s infinite; }
        </style>
        <div id="pulse-node" class="pulse">animated</div>
    "#;

    let mut doc = HtmlDocument::from_html(html, DocumentConfig::default());
    doc.resolve(0.0);

    let pulse_id = doc.get_element_by_id("pulse-node");
    if let Some(id) = pulse_id {
        doc.mutate().remove_and_drop_node(id);
    }

    // Must not panic: stale animation entry should be skipped safely.
    doc.resolve(0.1);
}

/// A finished `animation-fill-mode: forwards` animation must keep applying its
/// end values when an unrelated restyle (here `:hover`) happens afterwards.
#[test]
fn fill_mode_forwards_survives_restyle() {
    let html = r#"
        <style>
          @keyframes fade-in { from { opacity: 0; } to { opacity: 1; } }
          #menu { width: 100px; height: 100px; opacity: 0; animation: fade-in 0.3s forwards; }
          #menu:hover { color: red; }
        </style>
        <div id="menu"></div>
    "#;

    let mut doc = HtmlDocument::from_html(html, DocumentConfig::default());
    let opacity = |doc: &HtmlDocument| {
        let id = doc.get_element_by_id("menu").unwrap();
        doc.get_node(id)
            .unwrap()
            .primary_styles()
            .unwrap()
            .clone_opacity()
    };

    doc.resolve(0.0);
    doc.resolve(1.0);
    doc.resolve(2.0);
    assert_eq!(opacity(&doc), 1.0);

    doc.set_hover_to(10.0, 10.0);
    doc.resolve(3.0);
    assert_eq!(opacity(&doc), 1.0);
}
