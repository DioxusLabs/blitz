//! Tests for sub-document enumeration and picker coordinate translation:
//! each sub-document (e.g. a browser tab or iframe-like element) is an
//! inspectable target of its own.

use std::net::TcpStream;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use blitz_cdp_server::documents::{collect_document_ids, find_picking_document, with_document_in};
use blitz_cdp_server::{CdpServer, DevtoolsWaker, DocumentProvider, PickerEvent};
use blitz_dom::{BaseDocument, DocumentConfig};
use blitz_html::HtmlDocument;
use blitz_traits::shell::{ColorScheme, Viewport};
use serde_json::json;
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};

struct TestWaker(Mutex<Sender<()>>);
impl DevtoolsWaker for TestWaker {
    fn wake(&self) {
        let _ = self.0.lock().unwrap().send(());
    }
}

/// A document tree provider: exposes a root document and (recursively) its
/// sub-documents, the same way `blitz-shell`'s provider does
struct DocTreeProvider(BaseDocument);
impl DocumentProvider for DocTreeProvider {
    fn document_ids(&self) -> Vec<usize> {
        let mut ids = Vec::new();
        collect_document_ids(&self.0, &mut ids);
        ids
    }
    fn with_document(&mut self, id: usize, cb: &mut dyn FnMut(&mut BaseDocument)) {
        with_document_in(&mut self.0, id, cb);
    }
}

type Ws = WebSocket<MaybeTlsStream<TcpStream>>;

fn read_reply(ws: &mut Ws, id: u64, events: &mut Vec<serde_json::Value>) -> serde_json::Value {
    loop {
        let msg = ws.read().expect("read ws message");
        let Message::Text(text) = msg else { continue };
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid json");
        if parsed.get("id").and_then(|i| i.as_u64()) == Some(id) {
            assert!(
                parsed.get("error").is_none(),
                "command {id} failed: {parsed}"
            );
            return parsed["result"].clone();
        }
        if parsed.get("method").is_some() {
            events.push(parsed);
        }
    }
}

fn read_event(ws: &mut Ws, method: &str, events: &mut Vec<serde_json::Value>) -> serde_json::Value {
    loop {
        let msg = ws.read().expect("read ws message");
        let Message::Text(text) = msg else { continue };
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid json");
        if parsed.get("method").and_then(|m| m.as_str()) == Some(method) {
            return parsed["params"].clone();
        }
        if parsed.get("method").is_some() {
            events.push(parsed);
        }
    }
}

fn send_command(ws: &mut Ws, id: u64, method: &str, params: serde_json::Value) {
    let msg = json!({ "id": id, "method": method, "params": params });
    ws.send(Message::text(msg.to_string())).expect("send ws");
}

fn make_doc(html: &str) -> BaseDocument {
    let mut doc: BaseDocument = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            ..Default::default()
        },
    )
    .into();
    doc.resolve(0.0);
    doc
}

/// A parent document hosting a sub-document in an absolutely positioned
/// element at a known offset
fn make_doc_tree() -> (BaseDocument, usize) {
    let mut parent = make_doc(
        "<html><body style=\"margin: 0\">\
         <div id=\"host\" style=\"position: absolute; top: 100px; left: 50px; \
         width: 400px; height: 300px\"></div></body></html>",
    );
    let sub = make_doc(
        "<html><head><title>Sub Doc</title></head><body style=\"margin: 0\">\
         <p id=\"inner\" style=\"margin: 0; width: 100px; height: 20px\">Hi</p>\
         </body></html>",
    );
    let sub_id = sub.id();
    let host_id = parent.query_selector("#host").unwrap().unwrap();
    parent.set_sub_document(host_id, Box::new(sub));
    parent.resolve(0.0);
    (parent, sub_id)
}

#[test]
fn sub_document_helpers() {
    let (mut parent, sub_id) = make_doc_tree();
    let parent_id = parent.id();

    let mut ids = Vec::new();
    collect_document_ids(&parent, &mut ids);
    assert_eq!(ids, vec![parent_id, sub_id]);

    // with_document_in reaches the sub-document
    let mut found = None;
    with_document_in(&mut parent, sub_id, &mut |doc| found = Some(doc.id()));
    assert_eq!(found, Some(sub_id));

    // With no picker active, no document is found
    assert!(find_picking_document(&parent, 60.0, 110.0).is_none());

    // With the picker active on the parent, coordinates pass through
    parent.devtools_mut().element_picker = true;
    assert_eq!(
        find_picking_document(&parent, 60.0, 110.0),
        Some((parent_id, 60.0, 110.0))
    );
    parent.devtools_mut().element_picker = false;

    // With the picker active on the sub-document, window coordinates are
    // translated by the host element's position: the host is at (50, 100),
    // so window (60, 110) is (10, 10) in the sub-document
    with_document_in(&mut parent, sub_id, &mut |doc| {
        doc.devtools_mut().element_picker = true;
    });
    assert_eq!(
        find_picking_document(&parent, 60.0, 110.0),
        Some((sub_id, 10.0, 10.0))
    );
}

/// A sub-document is enumerated as its own target and a session attached to
/// it inspects the sub-document's DOM; picker events routed to it hit-test
/// in sub-document coordinates
#[test]
fn sub_document_session() {
    let (parent, sub_id) = make_doc_tree();
    let parent_id = parent.id();
    let mut provider = DocTreeProvider(parent);

    let (wake_sender, wake_receiver) = channel();
    let mut server = CdpServer::new(Arc::new(TestWaker(Mutex::new(wake_sender))));
    server.start_listening("127.0.0.1:0");
    let addr = server
        .wait_for_local_addr(Duration::from_secs(10))
        .expect("server should start listening");

    let (picker_sender, picker_receiver) = channel::<(f32, f32)>();
    let done = Arc::new(Mutex::new(false));
    std::thread::scope(|scope| {
        let done2 = Arc::clone(&done);
        scope.spawn(move || {
            struct DoneGuard(Arc<Mutex<bool>>);
            impl Drop for DoneGuard {
                fn drop(&mut self) {
                    *self.0.lock().unwrap() = true;
                }
            }
            let _guard = DoneGuard(done2);

            let (mut ws, _response) =
                tungstenite::connect(format!("ws://{addr}/devtools/page/{sub_id}"))
                    .expect("websocket connect");
            let mut events = Vec::new();

            send_command(&mut ws, 1, "DOM.enable", json!({}));
            read_reply(&mut ws, 1, &mut events);

            // The session inspects the sub-document, not the parent
            send_command(&mut ws, 2, "DOM.getDocument", json!({ "depth": -1 }));
            let result = read_reply(&mut ws, 2, &mut events);
            let html = result["root"].to_string();
            assert!(html.contains("inner"), "sub-document tree: {html}");
            assert!(!html.contains("host"), "sub-document tree: {html}");

            // Picker events carry sub-document-local coordinates (as
            // translated by find_picking_document): (10, 10) hits #inner
            send_command(
                &mut ws,
                3,
                "Overlay.setInspectMode",
                json!({ "mode": "searchForNode", "highlightConfig": {} }),
            );
            read_reply(&mut ws, 3, &mut events);
            picker_sender.send((10.0, 10.0)).unwrap();
            let params = read_event(&mut ws, "Overlay.nodeHighlightRequested", &mut events);
            let hovered_id = params["nodeId"].as_u64().unwrap();
            assert_ne!(hovered_id, 0);
            let node = &events
                .iter()
                .rev()
                .find_map(|e| {
                    if e["method"] != "DOM.setChildNodes" {
                        return None;
                    }
                    fn find(node: &serde_json::Value, id: u64) -> Option<&serde_json::Value> {
                        if node["nodeId"].as_u64() == Some(id) {
                            return Some(node);
                        }
                        node["children"]
                            .as_array()?
                            .iter()
                            .find_map(|child| find(child, id))
                    }
                    e["params"]["nodes"]
                        .as_array()?
                        .iter()
                        .find_map(|n| find(n, hovered_id))
                })
                .cloned();
            if let Some(node) = node {
                assert_eq!(node["nodeName"], "P");
            }
        });

        while !*done.lock().unwrap() {
            if wake_receiver
                .recv_timeout(Duration::from_millis(50))
                .is_ok()
            {
                server.process_messages(&mut provider);
            }
            while let Ok((x, y)) = picker_receiver.try_recv() {
                // Simulate the shell: window coordinates translated into the
                // picking (sub-)document's space by find_picking_document
                let mut event = None;
                provider.with_document(parent_id, &mut |doc| {
                    event = find_picking_document(doc, x + 50.0, y + 100.0)
                        .map(|(doc_id, x, y)| PickerEvent::Hovered { doc_id, x, y });
                });
                if let Some(event) = event {
                    server.notify_picker_event(event, &mut provider);
                }
            }
        }
    });
}
