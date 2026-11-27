use blitz_dom::{DocumentConfig, QualName, local_name, ns};
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

/// Animations on elements that become `display: none`, or whose ancestor does,
/// must stop so the document stops requesting animation frames.
#[test]
fn display_none_stops_animations() {
    let html = r#"
        <style>
          @keyframes pulse { 0% { opacity: 1; } 100% { opacity: 0.5; } }
          .pulse { animation: pulse 1s infinite; }
          .hidden { display: none; }
        </style>
        <div id="parent"><div class="pulse">animated</div></div>
        <div id="self" class="pulse">animated</div>
    "#;
    let class = || QualName::new(None, ns!(), local_name!("class"));

    let mut doc = HtmlDocument::from_html(html, DocumentConfig::default());
    doc.resolve(0.0);
    assert!(doc.is_animating());

    let parent = doc.get_element_by_id("parent").unwrap();
    doc.mutate().set_attribute(parent, class(), "hidden");
    let self_id = doc.get_element_by_id("self").unwrap();
    doc.mutate().set_attribute(self_id, class(), "pulse hidden");
    doc.resolve(0.1);
    doc.resolve(0.2);
    assert!(!doc.is_animating());
}
