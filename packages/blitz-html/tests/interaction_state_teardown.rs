//! When a node referenced by interaction state is removed, the usual teardown
//! steps must still run (matching browser semantics — WebKit
//! `hoveredElementDidDetach`/`elementInActiveChainDidDetach`, Blink
//! `HoveredElementDetached`/`ActiveChainNodeDetached`):
//!
//! - hover/active retarget to the nearest surviving element ancestor as a
//!   transient bridge, so the `:hover`/`:active` styling on the surviving
//!   chain never gaps; hover is then re-resolved against the (cached) pointer
//!   position at the end of the next resolve pass (the analogue of WebKit's
//!   "fake mouse move"), correcting the bridge value. Simply nulling the ids
//!   would orphan the element-state bits set along the ancestor chain,
//!   leaving stale `:hover`/`:active` styling.
//! - focus: resets to the body (encoded as `None`), running blur side-effects
//!   (in particular disabling IME for text inputs).

use blitz_dom::{Document, DocumentConfig, NodeId};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::{
    events::{
        BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, Point,
        PointerCoords, PointerDetails, UiEvent,
    },
    shell::{ColorScheme, ShellProvider, Viewport},
};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct RecordingShell {
    ime_enabled_calls: Mutex<Vec<bool>>,
}
impl ShellProvider for RecordingShell {
    fn set_ime_enabled(&self, is_enabled: bool) {
        self.ime_enabled_calls.lock().unwrap().push(is_enabled);
    }
}

fn make_doc(html: &str, shell: Option<Arc<RecordingShell>>) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 300, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            shell_provider: shell.map(|s| s as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn node_id(doc: &HtmlDocument, selector: &str) -> NodeId {
    doc.query_selector(selector).unwrap().expect(selector)
}

fn pointer_event(x: f32, y: f32, buttons: MouseEventButtons) -> BlitzPointerEvent {
    BlitzPointerEvent {
        id: BlitzPointerId::Mouse,
        is_primary: true,
        coords: PointerCoords {
            page_x: x,
            page_y: y,
            screen_x: x,
            screen_y: y,
            client_x: x,
            client_y: y,
        },
        button: MouseEventButton::Main,
        buttons,
        mods: Default::default(),
        details: PointerDetails::default(),
        element: Point::default(),
        active_pointers: Default::default(),
    }
}

const NESTED_HTML: &str = r#"<!DOCTYPE html>
<html><head><style> body { margin: 0; } </style></head>
<body><div id="container" style="width:200px; height:200px;"><div id="inner" style="height:100px;"></div></div></body></html>
"#;

#[test]
fn hover_bridges_to_surviving_ancestor_without_a_gap() {
    let mut doc = make_doc(NESTED_HTML, None);
    let container = node_id(&doc, "#container");
    let inner = node_id(&doc, "#inner");

    doc.set_hover_to(50.0, 50.0);
    assert_eq!(doc.get_hover_node_id(), Some(inner));
    assert!(doc.get_node(container).unwrap().is_hovered());

    // Removing the hovered node retargets hover to the surviving ancestor:
    // its :hover state must never gap (the pointer is still over it).
    doc.mutate().remove_and_drop_node(inner);
    assert_eq!(doc.get_hover_node_id(), Some(container));
    assert!(
        doc.get_node(container).unwrap().is_hovered(),
        ":hover must not gap on the surviving ancestor when the hovered node is removed"
    );

    // The next resolve re-resolves hover against the (cached) pointer
    // position, confirming the bridge value in this geometry.
    doc.resolve(0.0);
    assert_eq!(doc.get_hover_node_id(), Some(container));
    assert!(doc.get_node(container).unwrap().is_hovered());

    // Moving the pointer off the container clears its hover bit: the diff
    // works because the chain was bridged rather than nulled.
    doc.set_hover_to(300.0, 50.0);
    assert_ne!(doc.get_hover_node_id(), Some(container));
    assert!(
        !doc.get_node(container).unwrap().is_hovered(),
        "stale :hover element state left on ancestor after hovered node removal"
    );
}

/// The transient bridge value can be geometrically wrong (here the removed
/// node overflows its parent, so the pointer was never over the parent's
/// box). The post-resolve coordinate re-resolution must correct it.
#[test]
fn hover_bridge_is_corrected_by_coordinate_reresolution() {
    let mut doc = make_doc(
        r#"<!DOCTYPE html>
<html><head><style> body { margin: 0; } </style></head>
<body><div id="container" style="position:relative; width:100px; height:100px;"><div id="inner" style="position:absolute; left:200px; top:0; width:100px; height:100px;"></div></div></body></html>
"#,
        None,
    );
    let container = node_id(&doc, "#container");
    let inner = node_id(&doc, "#inner");

    // Hover the overflowing abspos child: the pointer is outside the
    // container's own box.
    doc.set_hover_to(250.0, 50.0);
    assert_eq!(doc.get_hover_node_id(), Some(inner));

    // The bridge transiently claims the container is hovered...
    doc.mutate().remove_and_drop_node(inner);
    assert_eq!(doc.get_hover_node_id(), Some(container));

    // ...but the next resolve re-resolves from the pointer position, which
    // is not over the container, and unhoveres it.
    doc.resolve(0.0);
    assert_ne!(doc.get_hover_node_id(), Some(container));
    assert!(
        !doc.get_node(container).unwrap().is_hovered(),
        "coordinate re-resolution must correct a geometrically-wrong bridge value"
    );
}

#[test]
fn active_state_bridges_mid_press_and_clears_on_release() {
    let mut doc = make_doc(NESTED_HTML, None);
    let container = node_id(&doc, "#container");
    let inner = node_id(&doc, "#inner");

    // Press on #inner: the whole ancestor chain becomes :active
    doc.handle_ui_event(UiEvent::PointerDown(pointer_event(
        50.0,
        50.0,
        MouseEventButtons::from(MouseEventButton::Main),
    )));
    assert!(doc.get_node(inner).unwrap().is_active());
    assert!(doc.get_node(container).unwrap().is_active());

    // Removing the active node mid-press bridges active state to the
    // surviving ancestor: its :active styling must not gap while the button
    // is still held.
    doc.mutate().remove_and_drop_node(inner);
    assert!(
        doc.get_node(container).unwrap().is_active(),
        ":active must not gap on the surviving ancestor while the press continues"
    );

    // Releasing the button clears :active from the surviving chain: this
    // only works because the chain was bridged rather than nulled
    // (`unactive_node` no-ops on a cleared id).
    doc.resolve(0.0);
    doc.handle_ui_event(UiEvent::PointerUp(pointer_event(
        50.0,
        50.0,
        MouseEventButtons::None,
    )));
    assert!(
        !doc.get_node(container).unwrap().is_active(),
        "stale :active element state left on ancestor after release"
    );
}

#[test]
fn ime_is_disabled_when_focused_text_input_is_removed() {
    let shell = Arc::new(RecordingShell::default());
    let mut doc = make_doc(
        r#"<!DOCTYPE html>
<html><head><style> body { margin: 0; } </style></head>
<body><input type="text" id="input"></body></html>
"#,
        Some(shell.clone()),
    );
    let input = node_id(&doc, "#input");

    doc.set_focus_to(input);
    assert_eq!(
        shell.ime_enabled_calls.lock().unwrap().last(),
        Some(&true),
        "focusing a text input should enable IME"
    );

    doc.mutate().remove_and_drop_node(input);
    assert_eq!(
        shell.ime_enabled_calls.lock().unwrap().last(),
        Some(&false),
        "removing the focused text input should run blur side-effects and disable IME"
    );
    // Focus falls back to the root element (get_focussed_node_id's default
    // when no node holds focus)
    assert_ne!(doc.get_focussed_node_id(), Some(input));
}
