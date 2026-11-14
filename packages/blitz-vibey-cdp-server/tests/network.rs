//! Tests for the `Network` domain: requests issued through an inspected
//! document's `NetProvider` (and its sub-documents') are reported to the
//! frontend, and their bodies can be read back

use std::net::TcpStream;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use blitz_dom::{BaseDocument, DocumentConfig};
use blitz_html::HtmlDocument;
use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request, Url};
use blitz_traits::shell::{ColorScheme, Viewport};
use blitz_vibey_cdp_server::documents::{collect_document_ids, with_document_in};
use blitz_vibey_cdp_server::{CdpServer, DevtoolsWaker, DocumentProvider};
use serde_json::json;
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};

struct TestWaker(Mutex<Sender<()>>);
impl DevtoolsWaker for TestWaker {
    fn wake(&self) {
        let _ = self.0.lock().unwrap().send(());
    }
}

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

/// Serves a fixed set of responses synchronously; other URLs fail
struct TestNetProvider;
impl NetProvider for TestNetProvider {
    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        let url = request.url.to_string();
        match request.url.path() {
            "/style.css" => handler.bytes(url, Bytes::from_static(b"p { color: red }")),
            "/image.png" => handler.bytes(url, Bytes::from_static(b"\x89PNG\r\n\x1a\nxyz")),
            _ => handler.error(url, "HTTP status 404 Not Found".to_string()),
        }
    }
}

struct NoopHandler;
impl NetHandler for NoopHandler {
    fn bytes(self: Box<Self>, _resolved_url: String, _bytes: Bytes) {}
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

/// Read events until `count` events with the given method have arrived
fn read_events(ws: &mut Ws, events: &mut Vec<serde_json::Value>, method: &str, count: usize) {
    while events
        .iter()
        .filter(|event| event["method"] == method)
        .count()
        < count
    {
        let msg = ws.read().expect("read ws message");
        let Message::Text(text) = msg else { continue };
        events.push(serde_json::from_str(&text).expect("valid json"));
    }
}

fn send_command(ws: &mut Ws, id: u64, method: &str, params: serde_json::Value) {
    let msg = json!({ "id": id, "method": method, "params": params });
    ws.send(Message::text(msg.to_string())).expect("send ws");
}

fn make_doc(html: &str, base_url: &str) -> BaseDocument {
    let mut doc: BaseDocument = HtmlDocument::from_html(
        html,
        DocumentConfig {
            base_url: Some(base_url.to_string()),
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            net_provider: Some(Arc::new(TestNetProvider)),
            ..Default::default()
        },
    )
    .into();
    doc.resolve(0.0);
    doc
}

/// Actions the client asks the embedder (test main loop) to perform
enum Action {
    /// Fetch a URL through the NetProvider of the given document
    Fetch(usize, &'static str),
    /// Check whether the given document's NetProvider is (still) the
    /// original one
    CheckRestored(usize, bool),
}

#[test]
fn network_events_and_bodies() {
    let mut parent = make_doc(
        "<html><body><div id=\"host\"></div></body></html>",
        "http://example.test/",
    );
    let sub = make_doc("<html><body></body></html>", "http://example.test/sub/");
    let sub_id = sub.id();
    let sub_original = Arc::clone(&sub.net_provider);
    let host_id = parent.query_selector("#host").unwrap().unwrap();
    parent.set_sub_document(host_id, Box::new(sub));
    let doc_id = parent.id();
    let original = Arc::clone(&parent.net_provider);
    let mut provider = DocTreeProvider(parent);

    let (wake_sender, wake_receiver) = channel();
    let mut server = CdpServer::new(Arc::new(TestWaker(Mutex::new(wake_sender.clone()))));
    server.start_listening("127.0.0.1:0");
    let addr = server
        .wait_for_local_addr(Duration::from_secs(10))
        .expect("server should start listening");

    let (action_sender, action_receiver) = channel::<Action>();
    let (checked_sender, checked_receiver) = channel::<bool>();
    let done = Arc::new(Mutex::new(false));
    std::thread::scope(|scope| {
        let done2 = Arc::clone(&done);
        scope.spawn(move || {
            struct DoneGuard(Arc<Mutex<bool>>, Sender<()>);
            impl Drop for DoneGuard {
                fn drop(&mut self) {
                    *self.0.lock().unwrap() = true;
                    let _ = self.1.send(());
                }
            }
            let _guard = DoneGuard(done2, wake_sender.clone());
            let act = |action| {
                action_sender.send(action).unwrap();
                wake_sender.send(()).unwrap();
            };

            let (mut ws, _) = tungstenite::connect(format!("ws://{addr}/devtools/page/{doc_id}"))
                .expect("websocket connect");
            let mut events = Vec::new();

            send_command(
                &mut ws,
                1,
                "Network.enable",
                json!({ "maxPostDataSize": 65536 }),
            );
            read_reply(&mut ws, 1, &mut events);
            // No-op commands are accepted
            send_command(
                &mut ws,
                2,
                "Network.setCacheDisabled",
                json!({ "cacheDisabled": true }),
            );
            read_reply(&mut ws, 2, &mut events);

            act(Action::Fetch(doc_id, "http://example.test/style.css"));
            act(Action::Fetch(sub_id, "http://example.test/image.png"));
            act(Action::Fetch(doc_id, "http://example.test/missing.css"));
            read_events(&mut ws, &mut events, "Network.loadingFinished", 2);
            read_events(&mut ws, &mut events, "Network.loadingFailed", 1);

            let sent: Vec<_> = events
                .iter()
                .filter(|event| event["method"] == "Network.requestWillBeSent")
                .map(|event| event["params"].clone())
                .collect();
            assert_eq!(sent.len(), 3);
            assert_eq!(sent[0]["request"]["url"], "http://example.test/style.css");
            assert_eq!(sent[0]["type"], "Stylesheet");
            assert_eq!(sent[0]["frameId"], format!("frame-{doc_id}"));
            assert_eq!(sent[0]["documentURL"], "http://example.test/");
            // Requests of sub-documents carry the sub-document's frame id
            assert_eq!(sent[1]["frameId"], format!("frame-{sub_id}"));
            assert_eq!(sent[1]["type"], "Image");

            let responses: Vec<_> = events
                .iter()
                .filter(|event| event["method"] == "Network.responseReceived")
                .map(|event| event["params"].clone())
                .collect();
            assert_eq!(responses.len(), 2);
            assert_eq!(responses[0]["requestId"], sent[0]["requestId"]);
            assert_eq!(responses[0]["response"]["status"], 200);
            assert_eq!(responses[0]["response"]["mimeType"], "text/css");
            assert_eq!(responses[1]["response"]["mimeType"], "image/png");

            let failed = events
                .iter()
                .find(|event| event["method"] == "Network.loadingFailed")
                .unwrap()
                .clone();
            assert_eq!(failed["params"]["requestId"], sent[2]["requestId"]);
            assert_eq!(failed["params"]["errorText"], "HTTP status 404 Not Found");

            send_command(
                &mut ws,
                3,
                "Network.getResponseBody",
                json!({ "requestId": sent[0]["requestId"] }),
            );
            let body = read_reply(&mut ws, 3, &mut events);
            assert_eq!(
                body,
                json!({ "body": "p { color: red }", "base64Encoded": false })
            );
            send_command(
                &mut ws,
                4,
                "Network.getResponseBody",
                json!({ "requestId": sent[1]["requestId"] }),
            );
            let body = read_reply(&mut ws, 4, &mut events);
            assert_eq!(
                body,
                json!({ "body": "iVBORw0KGgp4eXo=", "base64Encoded": true })
            );

            act(Action::CheckRestored(doc_id, false));
            assert!(checked_receiver.recv().unwrap());
            send_command(&mut ws, 5, "Network.disable", json!({}));
            read_reply(&mut ws, 5, &mut events);
            act(Action::CheckRestored(doc_id, true));
            assert!(checked_receiver.recv().unwrap());
            act(Action::CheckRestored(sub_id, true));
            assert!(checked_receiver.recv().unwrap());
        });

        while !*done.lock().unwrap() {
            if wake_receiver
                .recv_timeout(Duration::from_millis(50))
                .is_err()
            {
                continue;
            }
            server.process_messages(&mut provider);
            while let Ok(action) = action_receiver.try_recv() {
                match action {
                    Action::Fetch(id, url) => provider.with_document(id, &mut |doc| {
                        let request = Request::get(Url::parse(url).unwrap());
                        doc.net_provider
                            .fetch(doc.id(), request, Box::new(NoopHandler));
                    }),
                    Action::CheckRestored(id, expected) => {
                        let mut restored = false;
                        provider.with_document(id, &mut |doc| {
                            let original = if id == doc_id {
                                &original
                            } else {
                                &sub_original
                            };
                            restored = Arc::ptr_eq(&doc.net_provider, original);
                        });
                        let _ = checked_sender.send(restored == expected);
                    }
                }
            }
        }
    });
}

/// A sub-document target keeps recording as its host navigates: requests of
/// the document that replaced the recorded one are reported under its frame
#[test]
fn network_follows_sub_document_navigation() {
    let mut parent = make_doc(
        "<html><body><div id=\"host\"></div></body></html>",
        "http://example.test/",
    );
    let sub = make_doc("<html><body></body></html>", "http://example.test/a/");
    let host_id = parent.query_selector("#host").unwrap().unwrap();
    parent.set_sub_document(host_id, Box::new(sub));
    let target_id = format!("{}.{}", parent.id(), host_id.as_u64());
    let mut provider = DocTreeProvider(parent);

    let (wake_sender, wake_receiver) = channel();
    let mut server = CdpServer::new(Arc::new(TestWaker(Mutex::new(wake_sender.clone()))));
    server.start_listening("127.0.0.1:0");
    let addr = server
        .wait_for_local_addr(Duration::from_secs(10))
        .expect("server should start listening");

    let (navigate_sender, navigate_receiver) = channel::<()>();
    let (fetch_sender, fetch_receiver) = channel::<&'static str>();
    let next_doc_id = Arc::new(Mutex::new(None));
    let done = Arc::new(Mutex::new(false));
    std::thread::scope(|scope| {
        let done2 = Arc::clone(&done);
        let next_doc_id2 = Arc::clone(&next_doc_id);
        scope.spawn(move || {
            struct DoneGuard(Arc<Mutex<bool>>, Sender<()>);
            impl Drop for DoneGuard {
                fn drop(&mut self) {
                    *self.0.lock().unwrap() = true;
                    let _ = self.1.send(());
                }
            }
            let _guard = DoneGuard(done2, wake_sender.clone());

            let (mut ws, _) =
                tungstenite::connect(format!("ws://{addr}/devtools/page/{target_id}"))
                    .expect("websocket connect");
            let mut events = Vec::new();
            send_command(&mut ws, 1, "DOM.getDocument", json!({}));
            read_reply(&mut ws, 1, &mut events);
            send_command(&mut ws, 2, "Network.enable", json!({}));
            read_reply(&mut ws, 2, &mut events);

            navigate_sender.send(()).unwrap();
            wake_sender.send(()).unwrap();
            read_events(&mut ws, &mut events, "DOM.documentUpdated", 1);

            fetch_sender.send("http://example.test/style.css").unwrap();
            wake_sender.send(()).unwrap();
            read_events(&mut ws, &mut events, "Network.loadingFinished", 1);

            let sent = events
                .iter()
                .find(|event| event["method"] == "Network.requestWillBeSent")
                .unwrap();
            let next_doc_id = next_doc_id2.lock().unwrap().unwrap();
            assert_eq!(
                sent["params"]["request"]["url"],
                "http://example.test/style.css"
            );
            assert_eq!(sent["params"]["frameId"], format!("frame-{next_doc_id}"));
            assert_eq!(sent["params"]["documentURL"], "http://example.test/b/");
        });

        while !*done.lock().unwrap() {
            if wake_receiver
                .recv_timeout(Duration::from_millis(50))
                .is_err()
            {
                continue;
            }
            if navigate_receiver.try_recv().is_ok() {
                // Navigate the host: the recorded sub-document is dropped
                let next = make_doc("<html><body></body></html>", "http://example.test/b/");
                *next_doc_id.lock().unwrap() = Some(next.id());
                provider.0.set_sub_document(host_id, Box::new(next));
            }
            server.process_messages(&mut provider);
            while let Ok(url) = fetch_receiver.try_recv() {
                let doc_id = next_doc_id.lock().unwrap().unwrap();
                provider.with_document(doc_id, &mut |doc| {
                    let request = Request::get(Url::parse(url).unwrap());
                    doc.net_provider
                        .fetch(doc.id(), request, Box::new(NoopHandler));
                });
            }
        }
    });
}
