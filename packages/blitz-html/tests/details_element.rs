//! The <details>/<summary> disclosure widget. Activating the first <summary>
//! child toggles the `open` attribute of its <details> ancestor, and the
//! user-agent stylesheet hides everything except that summary while closed.

use blitz_dom::{Document, DocumentConfig, NodeId};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::{
    events::{
        BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, Point,
        PointerCoords, PointerDetails, UiEvent,
    },
    shell::{ColorScheme, Viewport},
};
use markup5ever::local_name;
use std::sync::Arc;

fn doc(html: &str) -> HtmlDocument {
    doc_scaled(html, 1.0)
}

fn doc_scaled(html: &str, scale: f32) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(
                (400.0 * scale) as u32,
                (400.0 * scale) as u32,
                scale,
                ColorScheme::Light,
            )),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn node_id(doc: &HtmlDocument, selector: &str) -> NodeId {
    doc.query_selector(selector).unwrap().expect(selector)
}

fn pointer_event(x: f32, y: f32) -> BlitzPointerEvent {
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
        buttons: MouseEventButtons::from(MouseEventButton::Main),
        mods: Default::default(),
        details: PointerDetails::default(),
        element: Point::default(),
        active_pointers: Default::default(),
    }
}

fn click(doc: &mut HtmlDocument, x: f32, y: f32) {
    let event = pointer_event(x, y);
    doc.handle_ui_event(UiEvent::PointerDown(event.clone()));
    doc.handle_ui_event(UiEvent::PointerUp(event));
    doc.resolve(0.0);
}

fn is_open(doc: &HtmlDocument, selector: &str) -> bool {
    let id = node_id(doc, selector);
    doc.get_node(id).unwrap().data.has_attr(local_name!("open"))
}

/// The content height of the details' body element. Zero when the details is
/// closed (the body is `display: none`), positive when open.
fn body_height(doc: &HtmlDocument, selector: &str) -> f32 {
    let id = node_id(doc, selector);
    doc.get_node(id).unwrap().final_layout.size.height
}

#[test]
fn clicking_summary_toggles_open() {
    let mut doc = doc(r#"<html><body style="margin:0">
        <details>
            <summary style="height:20px;">Summary</summary>
            <p id="body" style="height:40px;">Hidden content</p>
        </details>
    </body></html>"#);

    // Closed by default.
    assert!(!is_open(&doc, "details"));
    assert_eq!(body_height(&doc, "#body"), 0.0, "body hidden while closed");

    // Click on the summary (near the top-left, within the 20px summary row).
    click(&mut doc, 40.0, 10.0);
    assert!(is_open(&doc, "details"), "open after first click");
    assert!(body_height(&doc, "#body") > 0.0, "body visible while open");

    // Clicking again collapses it.
    click(&mut doc, 40.0, 10.0);
    assert!(!is_open(&doc, "details"), "closed after second click");
    assert_eq!(body_height(&doc, "#body"), 0.0, "body hidden again");
}

#[test]
fn summary_hit_area_matches_box_at_hidpi() {
    // `scrollable_overflow` is stored in device (scaled) pixels while hit-test
    // coordinates are CSS pixels. Before unscaling it in `Node::hit()`, every
    // element's hit area was inflated by the HiDPI scale factor, so at 2x the
    // summary's effective hit area was ~double its box (as seen on
    // https://gosub.io/faq) and clicks below it would still toggle the details.
    let mut doc = doc_scaled(
        r#"<html><body style="margin:0">
        <details open>
            <summary style="height:20px;">Summary</summary>
            <p id="body" style="height:40px; margin-top:40px;">Content</p>
        </details>
    </body></html>"#,
        2.0,
    );

    assert!(is_open(&doc, "details"));

    let summary = node_id(&doc, "summary");
    // (40, 30) is below the 20px summary, in the gap created by the content's
    // top margin. It must not hit-test as the summary.
    let hit = doc.hit(40.0, 30.0).expect("should hit the details");
    assert_ne!(
        hit.node_id, summary,
        "point below the summary must not hit the summary"
    );

    click(&mut doc, 40.0, 30.0);
    assert!(
        is_open(&doc, "details"),
        "clicking below the summary must not toggle the details"
    );
}

/// The text content of the given element's `::after` pseudo-element.
fn after_text(doc: &HtmlDocument, selector: &str) -> Option<String> {
    let id = node_id(doc, selector);
    let after_id = doc.get_node(id).unwrap().after?;
    let text_id = doc.get_node(after_id)?.children.first().copied()?;
    doc.get_node(text_id)?
        .text_data()
        .map(|t| t.content.clone())
}

#[test]
fn pseudo_element_content_updates_on_toggle() {
    // The `content` of a pseudo element is represented as a child text node of
    // the pseudo element's anonymous node. When the `content` style changes
    // (here via the `[open]` attribute toggling) the text must be updated.
    let mut doc = doc(r#"<html><head><style>
        summary::after { content: "+"; }
        details[open] > summary::after { content: "-"; }
    </style></head><body style="margin:0">
        <details>
            <summary style="height:20px;">Summary</summary>
            <p id="body" style="height:40px;">Content</p>
        </details>
    </body></html>"#);

    assert_eq!(after_text(&doc, "summary").as_deref(), Some("+"));

    // Open the details by clicking the summary
    click(&mut doc, 40.0, 10.0);
    assert!(is_open(&doc, "details"));
    assert_eq!(
        after_text(&doc, "summary").as_deref(),
        Some("-"),
        "::after content should update when the details is opened"
    );

    // And close it again
    click(&mut doc, 40.0, 10.0);
    assert!(!is_open(&doc, "details"));
    assert_eq!(
        after_text(&doc, "summary").as_deref(),
        Some("+"),
        "::after content should update when the details is closed"
    );
}

#[test]
fn details_starts_open_when_open_attribute_present() {
    let doc = doc(r#"<html><body style="margin:0">
        <details open>
            <summary style="height:20px;">Summary</summary>
            <p id="body" style="height:40px;">Visible content</p>
        </details>
    </body></html>"#);

    assert!(is_open(&doc, "details"));
    assert!(
        body_height(&doc, "#body") > 0.0,
        "body visible when initially open"
    );
}

#[test]
fn initially_open_details_can_be_closed() {
    // The `open` attribute created by the HTML parser lives in the empty
    // namespace. Toggling must remove that attribute (and not merely fail to
    // find a namespaced one).
    let mut doc = doc(r#"<html><body style="margin:0">
        <details open>
            <summary style="height:20px;">Summary</summary>
            <p id="body" style="height:40px;">Visible content</p>
        </details>
    </body></html>"#);

    assert!(is_open(&doc, "details"));

    // Click the summary to close it
    click(&mut doc, 40.0, 10.0);
    assert!(
        !is_open(&doc, "details"),
        "clicking the summary should close an initially-open details"
    );
    assert_eq!(body_height(&doc, "#body"), 0.0, "body hidden after closing");

    // And it can be re-opened
    click(&mut doc, 40.0, 10.0);
    assert!(is_open(&doc, "details"));
    assert!(body_height(&doc, "#body") > 0.0, "body visible again");
}
