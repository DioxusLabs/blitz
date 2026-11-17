use accesskit::{
    AriaCurrent, HasPopup, Invalid, Live, Node as AccessKitNode, NodeId, Orientation, Role,
    Toggled, TreeUpdate,
};
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn tree(body: &str) -> (HtmlDocument, TreeUpdate) {
    let mut document = HtmlDocument::from_html(
        &format!("<html><head></head><body>{body}</body></html>"),
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 2.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    document.resolve(0.0);
    let tree = document.build_accessibility_tree();
    (document, tree)
}

fn node<'a>(document: &HtmlDocument, tree: &'a TreeUpdate, id: &str) -> &'a AccessKitNode {
    let node_id = document.get_element_by_id(id).expect("no such id");
    let id = NodeId(node_id.as_u64());
    &tree
        .nodes
        .iter()
        .find(|(node, _)| *node == id)
        .expect("not in the tree")
        .1
}

#[test]
fn checked_and_pressed() {
    let (doc, tree) = tree(
        r#"<input id="native" type="checkbox" checked>
        <div id="mixed" role="checkbox" aria-checked="mixed"></div>
        <div id="switch" role="switch" aria-checked="false"></div>
        <button id="pressed" aria-pressed="true">Bold</button>
        <button id="plain">Save</button>"#,
    );
    assert_eq!(node(&doc, &tree, "native").toggled(), Some(Toggled::True));
    assert_eq!(node(&doc, &tree, "mixed").toggled(), Some(Toggled::Mixed));
    assert_eq!(node(&doc, &tree, "switch").role(), Role::Switch);
    assert_eq!(node(&doc, &tree, "switch").toggled(), Some(Toggled::False));
    assert_eq!(node(&doc, &tree, "pressed").toggled(), Some(Toggled::True));
    assert_eq!(node(&doc, &tree, "plain").toggled(), None);
}

#[test]
fn live_regions() {
    let (doc, tree) = tree(
        r#"<div id="status" role="status">Installing</div>
        <div id="alert" role="alert">Failed</div>
        <div id="polite" aria-live="polite" aria-atomic="false"></div>
        <div id="busy" aria-busy="true"></div>"#,
    );
    let status = node(&doc, &tree, "status");
    assert_eq!(status.live(), Some(Live::Polite));
    assert!(status.is_live_atomic());
    assert_eq!(node(&doc, &tree, "alert").live(), Some(Live::Assertive));
    assert_eq!(node(&doc, &tree, "polite").live(), Some(Live::Polite));
    assert!(!node(&doc, &tree, "polite").is_live_atomic());
    assert!(node(&doc, &tree, "busy").is_busy());
}

#[test]
fn values() {
    let (doc, tree) = tree(
        r#"<div id="slider" role="slider" aria-valuenow="30" aria-valuemin="0" aria-valuemax="50" aria-valuetext="30 percent" aria-orientation="horizontal"></div>
        <progress id="progress" value="0.25"></progress>
        <input id="text" value="Hollow" placeholder="Search">
        <input id="password" type="password" value="abc">"#,
    );
    let slider = node(&doc, &tree, "slider");
    assert_eq!(slider.numeric_value(), Some(30.0));
    assert_eq!(slider.min_numeric_value(), Some(0.0));
    assert_eq!(slider.max_numeric_value(), Some(50.0));
    assert_eq!(slider.value(), Some("30 percent"));
    assert_eq!(slider.orientation(), Some(Orientation::Horizontal));
    let progress = node(&doc, &tree, "progress");
    assert_eq!(progress.numeric_value(), Some(0.25));
    assert_eq!(progress.max_numeric_value(), Some(1.0));
    assert_eq!(node(&doc, &tree, "text").value(), Some("Hollow"));
    assert_eq!(node(&doc, &tree, "text").placeholder(), Some("Search"));
    assert_eq!(
        node(&doc, &tree, "password").value(),
        Some("\u{2022}\u{2022}\u{2022}")
    );
}

#[test]
fn properties_and_relations() {
    let (doc, tree) = tree(
        r#"<a id="link" href="/library" aria-current="page">Library</a>
        <div id="dialog" role="dialog" aria-modal="true"></div>
        <input id="field" required aria-invalid="true">
        <button id="menu" aria-haspopup="menu" aria-expanded="false" aria-controls="list">Sort</button>
        <div id="list" role="listbox" aria-activedescendant="second">
          <div id="first" role="option" aria-posinset="1" aria-setsize="2">A</div>
          <div id="second" role="option" aria-posinset="2" aria-setsize="2">B</div>
        </div>
        <div id="tokens" role="unknown-role button"></div>
        <button id="off" disabled>Off</button>"#,
    );
    let link = node(&doc, &tree, "link");
    assert_eq!(link.aria_current(), Some(AriaCurrent::Page));
    assert_eq!(link.url(), Some("/library"));
    assert!(node(&doc, &tree, "dialog").is_modal());
    let field = node(&doc, &tree, "field");
    assert!(field.is_required());
    assert_eq!(field.invalid(), Some(Invalid::True));
    let menu = node(&doc, &tree, "menu");
    assert_eq!(menu.has_popup(), Some(HasPopup::Menu));
    let list_id = NodeId(doc.get_element_by_id("list").unwrap().as_u64());
    assert_eq!(menu.controls(), &[list_id]);
    let second_id = NodeId(doc.get_element_by_id("second").unwrap().as_u64());
    assert_eq!(
        node(&doc, &tree, "list").active_descendant(),
        Some(second_id)
    );
    let second = node(&doc, &tree, "second");
    assert_eq!(second.position_in_set(), Some(2));
    assert_eq!(second.size_of_set(), Some(2));
    assert_eq!(node(&doc, &tree, "tokens").role(), Role::Button);
    let off = node(&doc, &tree, "off");
    assert!(off.is_disabled());
}
