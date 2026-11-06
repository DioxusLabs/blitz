//! Tests for recording the requests of documents built by the embedder with
//! a provider from `recording_net_provider`, and their source fetched by the
//! embedder (`DocumentLoadRecorder`). In their own test binary because they
//! record into the most recently created server.

use std::net::TcpStream;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use blitz_dom::{BaseDocument, DocumentConfig};
use blitz_html::HtmlDocument;
use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request, Url};
use blitz_traits::shell::{ColorScheme, Viewport};
use blitz_vibey_cdp_server::documents::{collect_document_ids, with_document_in};
use blitz_vibey_cdp_server::{
    CdpServer, DevtoolsWaker, DocumentLoadRecorder, DocumentProvider, recording_net_provider,
};
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

fn make_doc(html: &str, base_url: &str, net_provider: Arc<dyn NetProvider>) -> BaseDocument {
    let mut doc: BaseDocument = HtmlDocument::from_html(
        html,
        DocumentConfig {
            base_url: Some(base_url.to_string()),
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            net_provider: Some(net_provider),
            ..Default::default()
        },
    )
    .into();
    doc.resolve(0.0);
    doc
}

const PAGE_URL: &str = "http://example.test/b/page.html";
const PAGE_HTML: &str =
    "<html><head><link rel=\"stylesheet\" href=\"/style.css\"></head><body></body></html>";

/// Navigating a recorded host to a document built with the embedder's
/// recording provider reports the page load and the requests the document
/// made while being built (before it was shown), once each
#[test]
fn document_load_and_initial_requests_are_recorded() {
    let net_provider = recording_net_provider(Arc::new(TestNetProvider));
    let mut parent = make_doc(
        "<html><body><div id=\"host\"></div></body></html>",
        "http://example.test/",
        Arc::new(TestNetProvider),
    );
    let sub = make_doc(
        "<html><body></body></html>",
        "http://example.test/a/",
        Arc::clone(&net_provider),
    );
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
            read_events(&mut ws, &mut events, "Network.loadingFinished", 2);

            // A request made after the document is shown is reported once:
            // the server doesn't wrap the embedder's recording provider again
            fetch_sender.send("http://example.test/image.png").unwrap();
            wake_sender.send(()).unwrap();
            read_events(&mut ws, &mut events, "Network.loadingFinished", 3);

            let next_doc_id = next_doc_id2.lock().unwrap().unwrap();
            let sent: Vec<_> = events
                .iter()
                .filter(|event| event["method"] == "Network.requestWillBeSent")
                .map(|event| event["params"].clone())
                .collect();
            let urls: Vec<_> = sent.iter().map(|sent| &sent["request"]["url"]).collect();
            assert_eq!(
                urls,
                [
                    PAGE_URL,
                    "http://example.test/style.css",
                    "http://example.test/image.png"
                ]
            );
            assert_eq!(sent[0]["type"], "Document");
            assert_eq!(sent[1]["type"], "Stylesheet");
            for sent in &sent {
                assert_eq!(sent["frameId"], format!("frame-{next_doc_id}"));
                assert_eq!(sent["documentURL"], PAGE_URL);
            }

            let response = events
                .iter()
                .find(|event| event["method"] == "Network.responseReceived")
                .unwrap();
            assert_eq!(response["params"]["requestId"], sent[0]["requestId"]);
            assert_eq!(response["params"]["response"]["mimeType"], "text/html");
            send_command(
                &mut ws,
                3,
                "Network.getResponseBody",
                json!({ "requestId": sent[0]["requestId"] }),
            );
            let body = read_reply(&mut ws, 3, &mut events);
            assert_eq!(body, json!({ "body": PAGE_HTML, "base64Encoded": false }));
        });

        while !*done.lock().unwrap() {
            if wake_receiver
                .recv_timeout(Duration::from_millis(50))
                .is_err()
            {
                continue;
            }
            if navigate_receiver.try_recv().is_ok() {
                // Load the page as a browser does: fetch its source, build
                // the document, then show it in the host
                let request = Request::get(Url::parse(PAGE_URL).unwrap());
                let mut recorder = DocumentLoadRecorder::start(&request);
                recorder.response(PAGE_URL, &Bytes::from_static(PAGE_HTML.as_bytes()));
                let next = make_doc(PAGE_HTML, PAGE_URL, Arc::clone(&net_provider));
                *next_doc_id.lock().unwrap() = Some(next.id());
                recorder.finish(next.id());
                // The requests can't be sent before the document is shown
                server.process_messages(&mut provider);
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
