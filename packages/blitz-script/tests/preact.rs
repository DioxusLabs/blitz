//! End-to-end test: run the vendored Preact TodoMVC example (examples/preact)
//! headlessly and interact with it.

use std::path::PathBuf;

use blitz_dom::{Document, DocumentConfig};
use blitz_script::ScriptDocument;
use blitz_traits::events::{BlitzKeyEvent, DomEvent, KeyState, UiEvent};
use keyboard_types::{Code, Key, Location, Modifiers};
use url::Url;

fn preact_example_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/preact")
}

fn load_todomvc() -> ScriptDocument {
    let index_path = preact_example_dir()
        .join("index.html")
        .canonicalize()
        .unwrap();
    let html = std::fs::read_to_string(&index_path).unwrap();
    let base_url = Url::from_file_path(&index_path).unwrap();

    let mut doc = ScriptDocument::from_html(
        &html,
        DocumentConfig {
            base_url: Some(base_url.to_string()),
            ..Default::default()
        },
    );
    doc.execute_scripts();
    resolve(&mut doc);
    doc
}

/// Resolve style/layout. This constructs internal element state (text input
/// editors, checkbox state, ...) as would happen before rendering each frame
/// in a windowed application.
fn resolve(doc: &mut ScriptDocument) {
    doc.inner_mut().resolve(0.0);
}

fn query(doc: &ScriptDocument, selector: &str) -> Option<usize> {
    doc.inner().query_selector(selector).unwrap()
}

fn query_all(doc: &ScriptDocument, selector: &str) -> Vec<usize> {
    doc.inner().query_selector_all(selector).unwrap().to_vec()
}

fn text_of(doc: &ScriptDocument, node_id: usize) -> String {
    doc.inner().get_node(node_id).unwrap().text_content()
}

fn enter_key() -> BlitzKeyEvent {
    BlitzKeyEvent {
        key: Key::Enter,
        code: Code::Enter,
        modifiers: Modifiers::empty(),
        location: Location::Standard,
        is_auto_repeating: false,
        is_composing: false,
        state: KeyState::Pressed,
        text: None,
    }
}

fn click(doc: &mut ScriptDocument, selector: &str) {
    resolve(doc);
    let event = {
        let inner = doc.inner();
        let node_id = inner
            .query_selector(selector)
            .unwrap()
            .unwrap_or_else(|| panic!("no node matching {selector}"));
        DomEvent::new(
            node_id,
            inner
                .get_node(node_id)
                .unwrap()
                .synthetic_click_event(Modifiers::empty()),
        )
    };
    doc.dispatch_dom_event(event);
}

/// Type `text` into the new-todo input and press Enter
fn add_todo(doc: &mut ScriptDocument, text: &str) {
    resolve(doc);

    // Set the input's value (as if the user had typed it)
    doc.eval(&format!(
        "document.querySelector('.new-todo').value = {text:?};"
    ));

    // Focus the input and press Enter
    let input_id = query(doc, ".new-todo").expect("new-todo input");
    doc.inner_mut().set_focus_to(input_id);
    doc.handle_ui_event(UiEvent::KeyDown(enter_key()));
}

#[test]
fn renders_initial_ui() {
    let doc = load_todomvc();

    // Heading
    let h1 = query(&doc, ".app h1").expect("h1 should be rendered by Preact");
    assert_eq!(text_of(&doc, h1), "todos");

    // New-todo input with placeholder
    let input = query(&doc, "input.new-todo").expect("new-todo input");
    let inner = doc.inner();
    let placeholder = inner
        .get_node(input)
        .unwrap()
        .attr(blitz_dom::local_name!("placeholder"))
        .unwrap()
        .to_string();
    assert_eq!(placeholder, "What needs to be done?");
    drop(inner);

    // No todos yet: no list items and no footer
    assert!(query(&doc, "ul li").is_none());
    assert!(query(&doc, ".footer").is_none());
}

#[test]
fn add_toggle_filter_and_clear_todos() {
    let mut doc = load_todomvc();

    // === Add two todos ===
    add_todo(&mut doc, "Buy milk");
    add_todo(&mut doc, "Walk dog");

    let items = query_all(&doc, "ul > li");
    assert_eq!(items.len(), 2);
    assert!(text_of(&doc, items[0]).contains("Buy milk"));
    assert!(text_of(&doc, items[1]).contains("Walk dog"));

    // Input should have been cleared by the app's keydown handler
    let input_id = query(&doc, ".new-todo").unwrap();
    let value = {
        let inner = doc.inner();
        let node = inner.get_node(input_id).unwrap();
        node.element_data()
            .unwrap()
            .text_input_data()
            .unwrap()
            .editor
            .raw_text()
            .to_string()
    };
    assert_eq!(value, "");

    // Footer counter
    let footer = query(&doc, ".footer").expect("footer");
    assert!(text_of(&doc, footer).contains("2 items left"));

    // === Toggle the first todo via its checkbox ===
    click(&mut doc, "ul > li input[type='checkbox']");

    let done_items = query_all(&doc, "ul > li.done");
    assert_eq!(done_items.len(), 1);
    assert!(text_of(&doc, done_items[0]).contains("Buy milk"));
    let footer = query(&doc, ".footer").unwrap();
    assert!(text_of(&doc, footer).contains("1 item left"));

    // === Filter: "active" shows only the un-done todo ===
    click(&mut doc, ".filters button:nth-child(2)");
    let items = query_all(&doc, "ul > li");
    assert_eq!(items.len(), 1);
    assert!(text_of(&doc, items[0]).contains("Walk dog"));

    // === Filter: "completed" shows only the done todo ===
    click(&mut doc, ".filters button:nth-child(3)");
    let items = query_all(&doc, "ul > li");
    assert_eq!(items.len(), 1);
    assert!(text_of(&doc, items[0]).contains("Buy milk"));

    // === Back to "all" ===
    click(&mut doc, ".filters button:nth-child(1)");
    assert_eq!(query_all(&doc, "ul > li").len(), 2);

    // === Remove a todo with its destroy button ===
    click(&mut doc, "ul > li.done button.destroy");
    let items = query_all(&doc, "ul > li");
    assert_eq!(items.len(), 1);
    assert!(text_of(&doc, items[0]).contains("Walk dog"));

    // === Complete remaining todo and clear completed ===
    click(&mut doc, "ul > li input[type='checkbox']");
    let footer = query(&doc, ".footer").unwrap();
    assert!(text_of(&doc, footer).contains("0 items left"));

    click(&mut doc, "button.clear");
    assert!(query(&doc, "ul > li").is_none());
    // Footer disappears when there are no todos
    assert!(query(&doc, ".footer").is_none());
}
