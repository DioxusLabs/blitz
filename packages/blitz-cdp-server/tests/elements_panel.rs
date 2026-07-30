//! Integration test: connect to the CDP server over HTTP/WebSocket and
//! exercise the Elements-panel message sequence against a real document.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

struct SingleDocProvider(BaseDocument);
impl DocumentProvider for SingleDocProvider {
    fn document_ids(&self) -> Vec<usize> {
        vec![self.0.id()]
    }
    fn with_document(&mut self, id: usize, cb: &mut dyn FnMut(&mut BaseDocument)) {
        if id == self.0.id() {
            cb(&mut self.0);
        }
    }
}

type Ws = WebSocket<MaybeTlsStream<TcpStream>>;

/// Read WebSocket messages until a reply to the given command id arrives,
/// collecting any events received before it
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

/// Read WebSocket messages until an event with the given method arrives,
/// collecting other events into `events`
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

#[test]
fn elements_panel_session() {
    let html = "<html><head><title>Test Page</title></head>\
         <body><div id=\"container\" style=\"display: flex\">\
         <span class=\"a\">Hello</span> <span class=\"b\">World</span>\
         </div><p id=\"para\" style=\"width: 100px\">aaaa aaaa \
         <span id=\"wrapped\">bbbb bbbb bbbb bbbb</span> aaaa</p>\
         <div id=\"abs\" style=\"position: absolute; top: 20px; left: 30px; \
         width: 50px; height: 40px\"></div><div id=\"bb\" style=\"box-sizing: \
         border-box; width: 60px; height: 50px; padding: 5px; \
         border: 2px solid black\"></div><!-- marker comment --></body></html>";
    let mut doc: BaseDocument = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(1200, 800, 1.0, ColorScheme::Light)),
            ..Default::default()
        },
    )
    .into();
    doc.resolve(0.0);
    let mut provider = SingleDocProvider(doc);
    let doc_id = provider.0.id();

    let (wake_sender, wake_receiver) = channel();
    let mut server = CdpServer::new(Arc::new(TestWaker(Mutex::new(wake_sender))));
    // Port 0: let the OS assign a free port
    server.start_listening("127.0.0.1:0");
    let addr = server
        .wait_for_local_addr(Duration::from_secs(10))
        .expect("server should start listening");

    // Channel used by the client thread to simulate embedder-side element
    // picker input events (mouse moves/clicks while picking)
    let (picker_sender, picker_receiver) = channel::<PickerKind>();

    let done = Arc::new(Mutex::new(false));
    std::thread::scope(|scope| {
        let done2 = Arc::clone(&done);
        scope.spawn(move || {
            // Set the done flag even if the client panics, so the server
            // loop below doesn't spin forever on test failure
            struct DoneGuard(Arc<Mutex<bool>>);
            impl Drop for DoneGuard {
                fn drop(&mut self) {
                    *self.0.lock().unwrap() = true;
                }
            }
            let _guard = DoneGuard(done2);
            client_session(addr, doc_id, picker_sender);
        });

        while !*done.lock().unwrap() {
            if wake_receiver
                .recv_timeout(Duration::from_millis(50))
                .is_ok()
            {
                server.process_messages(&mut provider);
            }
            while let Ok(kind) = picker_receiver.try_recv() {
                let event = match kind {
                    PickerKind::Hovered(x, y) => PickerEvent::Hovered { doc_id, x, y },
                    PickerKind::Picked(x, y) => PickerEvent::Picked { doc_id, x, y },
                };
                server.notify_picker_event(event, &mut provider);
            }
        }
    });
}

/// Negative depths mean the entire subtree (per the CDP spec); positive
/// depths limit the number of levels returned
#[test]
fn subtree_depth() {
    let html = "<html><body><div id=\"outer\"><p id=\"mid\">\
         <span id=\"inner\"><b id=\"deep\">x</b></span></p></div></body></html>";
    let mut doc: BaseDocument = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(1200, 800, 1.0, ColorScheme::Light)),
            ..Default::default()
        },
    )
    .into();
    doc.resolve(0.0);
    let mut provider = SingleDocProvider(doc);
    let doc_id = provider.0.id();

    let (wake_sender, wake_receiver) = channel();
    let mut server = CdpServer::new(Arc::new(TestWaker(Mutex::new(wake_sender))));
    server.start_listening("127.0.0.1:0");
    let addr = server
        .wait_for_local_addr(Duration::from_secs(10))
        .expect("server should start listening");

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
            depth_client_session(addr, doc_id);
        });

        while !*done.lock().unwrap() {
            if wake_receiver
                .recv_timeout(Duration::from_millis(50))
                .is_ok()
            {
                server.process_messages(&mut provider);
            }
        }
    });
}

fn depth_client_session(addr: std::net::SocketAddr, doc_id: usize) {
    /// Find a node with the given "id" attribute anywhere in a `children`
    /// subtree
    fn find_by_id<'a>(node: &'a serde_json::Value, id: &str) -> Option<&'a serde_json::Value> {
        let attrs = node["attributes"].as_array();
        if attrs.is_some_and(|attrs| attrs.windows(2).any(|w| w[0] == "id" && w[1] == id)) {
            return Some(node);
        }
        node["children"]
            .as_array()?
            .iter()
            .find_map(|child| find_by_id(child, id))
    }

    let (mut ws, _response) =
        tungstenite::connect(format!("ws://{addr}/devtools/page/{doc_id}")).expect("ws connect");
    let mut events = Vec::new();

    // depth: -1 returns the entire subtree
    send_command(&mut ws, 1, "DOM.getDocument", json!({ "depth": -1 }));
    let result = read_reply(&mut ws, 1, &mut events);
    let deep = find_by_id(&result["root"], "deep").expect("deep node in full subtree");
    assert_eq!(deep["nodeName"], "B");

    // Reconnect for a fresh session (children are only sent once per session)
    let (mut ws, _response) =
        tungstenite::connect(format!("ws://{addr}/devtools/page/{doc_id}")).expect("ws connect");

    // depth: 1 returns a single level
    send_command(&mut ws, 1, "DOM.getDocument", json!({ "depth": 1 }));
    let result = read_reply(&mut ws, 1, &mut events);
    let root = &result["root"];
    let html_node = &root["children"][0];
    assert_eq!(html_node["nodeName"], "HTML");
    assert!(html_node["children"].as_array().is_none());
    assert!(find_by_id(root, "outer").is_none());

    // requestChildNodes with depth: -1 sends the node's entire subtree
    let html_id = html_node["nodeId"].as_u64().unwrap();
    send_command(
        &mut ws,
        2,
        "DOM.requestChildNodes",
        json!({ "nodeId": html_id, "depth": -1 }),
    );
    read_reply(&mut ws, 2, &mut events);
    let set_children = events
        .iter()
        .find(|event| event["method"] == "DOM.setChildNodes")
        .expect("DOM.setChildNodes event");
    assert_eq!(set_children["params"]["parentId"], html_id);
    let deep = set_children["params"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|node| find_by_id(node, "deep"))
        .expect("deep node in full subtree");
    assert_eq!(deep["nodeName"], "B");
    events.clear();

    // A repeated requestChildNodes does not resend already-sent children
    send_command(
        &mut ws,
        3,
        "DOM.requestChildNodes",
        json!({ "nodeId": html_id, "depth": -1 }),
    );
    read_reply(&mut ws, 3, &mut events);
    assert!(!events.iter().any(|e| e["method"] == "DOM.setChildNodes"));
}

/// A target id supplied in the WebSocket path is fixed for the session's
/// lifetime: if that document does not exist (e.g. it has closed), commands
/// fail rather than falling back to another document
#[test]
fn unknown_target_id_errors() {
    let html = "<html><body><div>Hello</div></body></html>";
    let mut doc: BaseDocument = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(1200, 800, 1.0, ColorScheme::Light)),
            ..Default::default()
        },
    )
    .into();
    doc.resolve(0.0);
    let mut provider = SingleDocProvider(doc);
    let doc_id = provider.0.id();

    let (wake_sender, wake_receiver) = channel();
    let mut server = CdpServer::new(Arc::new(TestWaker(Mutex::new(wake_sender))));
    server.start_listening("127.0.0.1:0");
    let addr = server
        .wait_for_local_addr(Duration::from_secs(10))
        .expect("server should start listening");

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

            // A session bound to a nonexistent document id errors
            let (mut ws, _response) =
                tungstenite::connect(format!("ws://{addr}/devtools/page/{}", doc_id + 1))
                    .expect("ws connect");
            send_command(&mut ws, 1, "DOM.getDocument", json!({ "depth": 1 }));
            loop {
                let msg = ws.read().expect("read ws message");
                let Message::Text(text) = msg else { continue };
                let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid json");
                if parsed.get("id").and_then(|i| i.as_u64()) == Some(1) {
                    assert_eq!(parsed["error"]["message"], "No document");
                    break;
                }
            }
        });

        while !*done.lock().unwrap() {
            if wake_receiver
                .recv_timeout(Duration::from_millis(50))
                .is_ok()
            {
                server.process_messages(&mut provider);
            }
        }
    });
}

/// Closing a connection must clear any inspect-mode/highlight state its
/// session set on the document (e.g. DevTools window closed mid-picking)
#[test]
fn connection_close_clears_devtools_state() {
    let html = "<html><body><div id=\"d\">Hello</div></body></html>";
    let mut doc: BaseDocument = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(1200, 800, 1.0, ColorScheme::Light)),
            ..Default::default()
        },
    )
    .into();
    doc.resolve(0.0);
    let mut provider = SingleDocProvider(doc);
    let doc_id = provider.0.id();

    let (wake_sender, wake_receiver) = channel();
    let mut server = CdpServer::new(Arc::new(TestWaker(Mutex::new(wake_sender))));
    server.start_listening("127.0.0.1:0");
    let addr = server
        .wait_for_local_addr(Duration::from_secs(10))
        .expect("server should start listening");

    let mut pump = |server: &mut CdpServer,
                    provider: &mut SingleDocProvider,
                    until: &dyn Fn(&SingleDocProvider) -> bool| {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !until(provider) {
            assert!(std::time::Instant::now() < deadline, "timed out waiting");
            if wake_receiver
                .recv_timeout(Duration::from_millis(50))
                .is_ok()
            {
                server.process_messages(provider);
            }
        }
    };

    let (mut ws, _response) =
        tungstenite::connect(format!("ws://{addr}/devtools/page/{doc_id}")).expect("ws connect");

    // Enter inspect mode and highlight a node during hover
    send_command(
        &mut ws,
        1,
        "Overlay.setInspectMode",
        json!({ "mode": "searchForNode", "highlightConfig": {} }),
    );
    pump(&mut server, &mut provider, &|p| {
        p.0.devtools().element_picker
    });
    server.notify_picker_event(
        PickerEvent::Hovered {
            doc_id,
            x: 10.0,
            y: 10.0,
        },
        &mut provider,
    );
    assert!(provider.0.devtools().highlight_node.is_some());

    // Closing the connection clears the state left on the document
    ws.close(None).expect("close ws");
    pump(&mut server, &mut provider, &|p| {
        !p.0.devtools().element_picker && p.0.devtools().highlight_node.is_none()
    });
}

enum PickerKind {
    Hovered(f32, f32),
    Picked(f32, f32),
}

fn client_session(addr: std::net::SocketAddr, doc_id: usize, picker_sender: Sender<PickerKind>) {
    // HTTP discovery: /json/list should advertise our document
    let mut http = TcpStream::connect(addr).expect("connect for /json/list");
    http.write_all(format!("GET /json/list HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
        .unwrap();
    let mut response = String::new();
    http.read_to_string(&mut response).unwrap();
    let body = response.split("\r\n\r\n").nth(1).expect("http body");
    let list: serde_json::Value = serde_json::from_str(body).expect("valid /json/list body");
    let targets = list.as_array().expect("target list");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0]["title"], "Test Page");
    assert_eq!(targets[0]["type"], "page");
    let ws_url = targets[0]["webSocketDebuggerUrl"]
        .as_str()
        .expect("webSocketDebuggerUrl");
    assert!(ws_url.ends_with(&format!("/devtools/page/{doc_id}")));
    // Must be the regular devtools app (not the screencast app, which routes
    // inspect mode away from the Overlay domain) and must use 127.0.0.1
    // (the frontend's CSP silently blocks `localhost` websockets)
    let frontend_url = targets[0]["devtoolsFrontendUrl"]
        .as_str()
        .expect("devtoolsFrontendUrl");
    assert!(
        frontend_url.starts_with("devtools://devtools/bundled/devtools_app.html?ws=127.0.0.1:")
    );

    // HTTP discovery: /json/version
    let mut http = TcpStream::connect(addr).expect("connect for /json/version");
    http.write_all(format!("GET /json/version HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
        .unwrap();
    let mut response = String::new();
    http.read_to_string(&mut response).unwrap();
    let body = response.split("\r\n\r\n").nth(1).expect("http body");
    let version: serde_json::Value = serde_json::from_str(body).expect("valid /json/version body");
    assert_eq!(version["Browser"], "Blitz");

    // WebSocket CDP session
    let (mut ws, _response) = tungstenite::connect(ws_url).expect("websocket connect");
    let mut events = Vec::new();
    let mut next_id = 0u64;
    let mut send = |ws: &mut Ws, method: &str, params: serde_json::Value| -> u64 {
        next_id += 1;
        send_command(ws, next_id, method, params);
        next_id
    };

    // Frontend startup: enables and stubs must not error
    let id = send(&mut ws, "Page.enable", json!({}));
    read_reply(&mut ws, id, &mut events);
    let id = send(&mut ws, "Runtime.enable", json!({}));
    read_reply(&mut ws, id, &mut events);
    let id = send(&mut ws, "DOM.enable", json!({}));
    read_reply(&mut ws, id, &mut events);
    let id = send(&mut ws, "CSS.enable", json!({}));
    read_reply(&mut ws, id, &mut events);
    let id = send(&mut ws, "Overlay.enable", json!({}));
    read_reply(&mut ws, id, &mut events);
    let id = send(
        &mut ws,
        "Target.setAutoAttach",
        json!({ "autoAttach": true, "waitForDebuggerOnStart": false, "flatten": true }),
    );
    read_reply(&mut ws, id, &mut events);

    // Page.getResourceTree provides the frame tree
    let id = send(&mut ws, "Page.getResourceTree", json!({}));
    let result = read_reply(&mut ws, id, &mut events);
    assert!(result["frameTree"]["frame"]["id"].is_string());

    // DOM.getDocument
    let id = send(&mut ws, "DOM.getDocument", json!({ "depth": 2 }));
    let result = read_reply(&mut ws, id, &mut events);
    let root = &result["root"];
    assert_eq!(root["nodeType"], 9);
    assert_eq!(root["nodeName"], "#document");
    let html_node = &root["children"][0];
    assert_eq!(html_node["nodeName"], "HTML");
    let body = html_node["children"]
        .as_array()
        .unwrap()
        .iter()
        .find(|child| child["nodeName"] == "BODY")
        .expect("body node");
    let body_id = body["nodeId"].as_u64().unwrap();

    // DOM.requestChildNodes emits DOM.setChildNodes before the reply
    let id = send(
        &mut ws,
        "DOM.requestChildNodes",
        json!({ "nodeId": body_id }),
    );
    read_reply(&mut ws, id, &mut events);
    let set_children = events
        .iter()
        .find(|event| event["method"] == "DOM.setChildNodes")
        .expect("DOM.setChildNodes event");
    assert_eq!(set_children["params"]["parentId"], body_id);
    let body_children = set_children["params"]["nodes"].as_array().unwrap();
    assert_eq!(body_children.len(), 5);
    assert_eq!(body_children[0]["nodeName"], "DIV");
    let comment = &body_children[4];
    assert_eq!(comment["nodeName"], "#comment");
    assert_eq!(comment["nodeValue"], " marker comment ");
    events.clear();

    // DOM.querySelector for the flex container
    let root_id = root["nodeId"].as_u64().unwrap();
    let id = send(
        &mut ws,
        "DOM.querySelector",
        json!({ "nodeId": root_id, "selector": "#container" }),
    );
    let result = read_reply(&mut ws, id, &mut events);
    let container_id = result["nodeId"].as_u64().unwrap();
    assert_ne!(container_id, 0);
    events.clear();

    // CSS.getMatchedStylesForNode: the inline style attribute should be
    // reported as the inlineStyle
    let id = send(
        &mut ws,
        "CSS.getMatchedStylesForNode",
        json!({ "nodeId": container_id }),
    );
    let result = read_reply(&mut ws, id, &mut events);
    let inline_props = result["inlineStyle"]["cssProperties"].as_array().unwrap();
    assert!(
        inline_props
            .iter()
            .any(|p| p["name"] == "display" && p["value"] == "flex")
    );
    // Matched rules should include user-agent rules (e.g. for div)
    let matched = result["matchedCSSRules"].as_array().unwrap();
    assert!(matched.iter().any(|m| {
        m["rule"]["origin"] == "user-agent"
            && m["rule"]["selectorList"]["text"]
                .as_str()
                .unwrap()
                .contains("div")
    }));
    // Inherited entries should be present (body, html ancestors)
    assert!(!result["inherited"].as_array().unwrap().is_empty());

    // CSS.getComputedStyleForNode
    let id = send(
        &mut ws,
        "CSS.getComputedStyleForNode",
        json!({ "nodeId": container_id }),
    );
    let result = read_reply(&mut ws, id, &mut events);
    let computed = result["computedStyle"].as_array().unwrap();
    assert!(
        computed
            .iter()
            .any(|p| p["name"] == "display" && p["value"] == "flex")
    );
    // width/height report used (post-layout) pixel values, not the
    // specified `auto`, so the Box Model diagram shows real dimensions
    for name in ["width", "height"] {
        let value = computed
            .iter()
            .find(|p| p["name"] == name)
            .and_then(|p| p["value"].as_str())
            .unwrap();
        assert!(value.ends_with("px"), "{name} should be in px: {value}");
        assert!(value.trim_end_matches("px").parse::<f64>().unwrap() > 0.0);
    }

    // Inset properties resolve to used px values for positioned elements
    let id = send(
        &mut ws,
        "DOM.querySelector",
        json!({ "nodeId": root_id, "selector": "#abs" }),
    );
    let result = read_reply(&mut ws, id, &mut events);
    let abs_id = result["nodeId"].as_u64().unwrap();
    assert_ne!(abs_id, 0);
    events.clear();
    let id = send(
        &mut ws,
        "CSS.getComputedStyleForNode",
        json!({ "nodeId": abs_id }),
    );
    let result = read_reply(&mut ws, id, &mut events);
    let computed = result["computedStyle"].as_array().unwrap();
    for (name, expected) in [("top", "20px"), ("left", "30px"), ("width", "50px")] {
        let value = computed
            .iter()
            .find(|p| p["name"] == name)
            .and_then(|p| p["value"].as_str())
            .unwrap();
        assert_eq!(value, expected, "used value for {name}");
    }

    // Reported width/height are box-sizing aware: border-box elements
    // report their border-box size
    let id = send(
        &mut ws,
        "DOM.querySelector",
        json!({ "nodeId": root_id, "selector": "#bb" }),
    );
    let result = read_reply(&mut ws, id, &mut events);
    let bb_id = result["nodeId"].as_u64().unwrap();
    assert_ne!(bb_id, 0);
    events.clear();
    let id = send(
        &mut ws,
        "CSS.getComputedStyleForNode",
        json!({ "nodeId": bb_id }),
    );
    let result = read_reply(&mut ws, id, &mut events);
    let computed = result["computedStyle"].as_array().unwrap();
    for (name, expected) in [("width", "60px"), ("height", "50px")] {
        let value = computed
            .iter()
            .find(|p| p["name"] == name)
            .and_then(|p| p["value"].as_str())
            .unwrap();
        assert_eq!(value, expected, "border-box used value for {name}");
    }

    // CSS.getInlineStylesForNode
    let id = send(
        &mut ws,
        "CSS.getInlineStylesForNode",
        json!({ "nodeId": container_id }),
    );
    let result = read_reply(&mut ws, id, &mut events);
    let inline_props = result["inlineStyle"]["cssProperties"].as_array().unwrap();
    assert!(inline_props.iter().any(|p| p["name"] == "display"));

    // DOM.getBoxModel
    let id = send(
        &mut ws,
        "DOM.getBoxModel",
        json!({ "nodeId": container_id }),
    );
    let result = read_reply(&mut ws, id, &mut events);
    let model = &result["model"];
    assert_eq!(model["border"].as_array().unwrap().len(), 8);
    assert!(model["width"].as_i64().unwrap() > 0);
    assert!(model["height"].as_i64().unwrap() > 0);

    // A non-atomic inline element that wraps across line boxes has no layout
    // box of its own: getBoxModel must report the bounding rect of its
    // line-box fragments
    let id = send(
        &mut ws,
        "DOM.querySelector",
        json!({ "nodeId": root_id, "selector": "#wrapped" }),
    );
    let result = read_reply(&mut ws, id, &mut events);
    let wrapped_id = result["nodeId"].as_u64().unwrap();
    assert_ne!(wrapped_id, 0);
    events.clear();

    let id = send(&mut ws, "DOM.getBoxModel", json!({ "nodeId": wrapped_id }));
    let result = read_reply(&mut ws, id, &mut events);
    let model = &result["model"];
    assert!(model["width"].as_i64().unwrap() > 0);
    assert!(model["height"].as_i64().unwrap() > 0);

    // Overlay.highlightNode / hideHighlight
    let id = send(
        &mut ws,
        "Overlay.highlightNode",
        json!({ "nodeId": container_id, "highlightConfig": { "contentColor": { "r": 111, "g": 168, "b": 220, "a": 0.66 } } }),
    );
    read_reply(&mut ws, id, &mut events);
    let id = send(&mut ws, "Overlay.hideHighlight", json!({}));
    read_reply(&mut ws, id, &mut events);

    // Page.startScreencast (sent by the chrome://inspect screencast app)
    // reports the screencast as not visible, since frames are not supported
    let id = send(
        &mut ws,
        "Page.startScreencast",
        json!({ "format": "jpeg", "quality": 80, "maxWidth": 800, "maxHeight": 600 }),
    );
    read_reply(&mut ws, id, &mut events);
    assert!(
        events
            .iter()
            .any(|e| e["method"] == "Page.screencastVisibilityChanged"
                && e["params"]["visible"] == false)
    );
    events.clear();

    // Element picker: after setInspectMode, simulated mouse events should
    // produce nodeHighlightRequested/inspectNodeRequested events
    let id = send(
        &mut ws,
        "Overlay.setInspectMode",
        json!({ "mode": "searchForNode", "highlightConfig": {} }),
    );
    read_reply(&mut ws, id, &mut events);

    picker_sender.send(PickerKind::Hovered(10.0, 10.0)).unwrap();
    let params = read_event(&mut ws, "Overlay.nodeHighlightRequested", &mut events);
    let hovered_id = params["nodeId"].as_u64().unwrap();
    assert_ne!(hovered_id, 0);
    // The hovered node's ancestor path is described (before the highlight
    // event) via setChildNodes, so the frontend can reveal it in realtime
    assert!(events.iter().any(|e| e["method"] == "DOM.setChildNodes"));
    events.clear();

    picker_sender.send(PickerKind::Picked(10.0, 10.0)).unwrap();
    let params = read_event(&mut ws, "Overlay.inspectNodeRequested", &mut events);
    let backend_id = params["backendNodeId"].as_u64().unwrap();
    assert_eq!(backend_id, hovered_id);

    // Frontend reveals the picked node
    let id = send(
        &mut ws,
        "DOM.pushNodesByBackendIdsToFrontend",
        json!({ "backendNodeIds": [backend_id] }),
    );
    let result = read_reply(&mut ws, id, &mut events);
    assert_eq!(result["nodeIds"][0].as_u64().unwrap(), backend_id);
    // The node's ancestor path was already sent while hovering, so it is
    // not resent here
    assert!(!events.iter().any(|e| e["method"] == "DOM.setChildNodes"));
    events.clear();

    // Pushing the same node again must not resend children the frontend
    // already knows (that would replace its node objects and break the
    // tree's selection/expansion state)
    let id = send(
        &mut ws,
        "DOM.pushNodesByBackendIdsToFrontend",
        json!({ "backendNodeIds": [backend_id] }),
    );
    let result = read_reply(&mut ws, id, &mut events);
    assert_eq!(result["nodeIds"][0].as_u64().unwrap(), backend_id);
    assert!(!events.iter().any(|e| e["method"] == "DOM.setChildNodes"));
    events.clear();

    // Turning inspect mode off after picking is a no-op
    let id = send(
        &mut ws,
        "Overlay.setInspectMode",
        json!({ "mode": "none", "highlightConfig": {} }),
    );
    read_reply(&mut ws, id, &mut events);

    // Unknown methods produce a method-not-found error, not a hang
    next_id += 1;
    send_command(&mut ws, next_id, "Bogus.method", json!({}));
    loop {
        let msg = ws.read().expect("read ws message");
        let Message::Text(text) = msg else { continue };
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("valid json");
        if parsed.get("id").and_then(|i| i.as_u64()) == Some(next_id) {
            assert_eq!(parsed["error"]["code"], -32601);
            break;
        }
    }
}
