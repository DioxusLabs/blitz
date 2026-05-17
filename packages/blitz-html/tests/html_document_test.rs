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
