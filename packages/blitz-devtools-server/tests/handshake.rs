//! Integration test: connect to the devtools server over TCP and exercise
//! the session-initialization message sequence against a real document.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use blitz_devtools_server::{DevtoolsServer, DevtoolsWaker, DocumentProvider, PickerEvent};
use blitz_dom::{BaseDocument, DocumentConfig};
use blitz_html::HtmlDocument;
use blitz_traits::shell::{ColorScheme, Viewport};

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

/// Read a single `length:{json}` packet from the stream
fn read_packet(stream: &mut TcpStream) -> serde_json::Value {
    let mut header = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        stream.read_exact(&mut byte).expect("read header byte");
        if byte[0] == b':' {
            break;
        }
        header.push(byte[0]);
        assert!(header.len() < 20, "header too long");
    }
    let len: usize = str::from_utf8(&header).unwrap().parse().unwrap();
    let mut data = vec![0u8; len];
    stream.read_exact(&mut data).expect("read packet body");
    serde_json::from_slice(&data).expect("valid json packet")
}

fn write_packet(stream: &mut TcpStream, packet: serde_json::Value) {
    let encoded = serde_json::to_string(&packet).unwrap();
    stream
        .write_all(format!("{}:{}", encoded.len(), encoded).as_bytes())
        .unwrap();
}

#[test]
fn session_initialization() {
    let html = "<html><head><title>Test Page</title></head>\
         <body><div id=\"container\" style=\"display: flex\">\
         <span class=\"a\">Hello</span> <span class=\"b\">World</span>\
         </div><p id=\"para\" style=\"width: 100px\">aaaa aaaa \
         <span id=\"wrapped\">bbbb bbbb bbbb bbbb</span> aaaa</p></body></html>";
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

    let (wake_sender, wake_receiver) = channel();
    let mut server = DevtoolsServer::new(Arc::new(TestWaker(Mutex::new(wake_sender))));
    // Port 0: let the OS assign a free port
    server.start_listening("127.0.0.1:0");
    let addr = server
        .wait_for_local_addr(Duration::from_secs(10))
        .expect("server should start listening");

    let stream = TcpStream::connect(addr).expect("connect to devtools server");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();

    // Run the client on a separate thread; process messages when woken on
    // this thread (which owns the document), simulating the embedder's
    // event loop. `BaseDocument` is not `Send`, so it must stay here.
    // Channel used by the client thread to simulate embedder-side element
    // picker input events (mouse moves/clicks while picking)
    let (picker_sender, picker_receiver) = channel::<PickerKind>();
    let doc_id = provider.0.id();

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
            client_session(stream, picker_sender);
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

enum PickerKind {
    Hovered(f32, f32),
    Picked(f32, f32),
}

fn client_session(mut stream: TcpStream, picker_sender: Sender<PickerKind>) {
    {
        // Initial notification from the root actor
        let msg = read_packet(&mut stream);
        assert_eq!(msg["from"], "root");
        assert_eq!(msg["applicationType"], "browser");

        // getRoot
        write_packet(
            &mut stream,
            serde_json::json!({ "to": "root", "type": "getRoot" }),
        );
        let msg = read_packet(&mut stream);
        assert_eq!(msg["from"], "root");
        assert!(msg["deviceActor"].is_string());

        // listTabs should return our document
        write_packet(
            &mut stream,
            serde_json::json!({ "to": "root", "type": "listTabs" }),
        );
        let msg = read_packet(&mut stream);
        let tabs = msg["tabs"].as_array().expect("tabs array");
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0]["title"], "Test Page");
        let tab_actor = tabs[0]["actor"].as_str().expect("tab actor name");

        // getWatcher
        write_packet(
            &mut stream,
            serde_json::json!({ "to": tab_actor, "type": "getWatcher", "isServerTargetSwitchingEnabled": true }),
        );
        let msg = read_packet(&mut stream);
        let watcher_actor = msg["actor"]
            .as_str()
            .expect("watcher actor name")
            .to_string();

        // watchTargets: expect a target-available-form notification then an
        // empty reply
        write_packet(
            &mut stream,
            serde_json::json!({ "to": watcher_actor, "type": "watchTargets", "targetType": "frame" }),
        );
        let msg = read_packet(&mut stream);
        assert_eq!(msg["type"], "target-available-form");
        let target = &msg["target"];
        assert_eq!(target["title"], "Test Page");
        let inspector_actor = target["inspectorActor"].as_str().unwrap().to_string();
        let css_properties_actor = target["cssPropertiesActor"].as_str().unwrap().to_string();
        let _ = read_packet(&mut stream); // empty reply

        // getCSSDatabase
        write_packet(
            &mut stream,
            serde_json::json!({ "to": css_properties_actor, "type": "getCSSDatabase" }),
        );
        let msg = read_packet(&mut stream);
        let properties = msg["properties"].as_object().expect("css properties");
        assert!(properties.contains_key("display"));
        assert!(properties.contains_key("flex-direction"));
        assert_eq!(properties["color"]["isInherited"], true);
        assert_eq!(properties["display"]["isInherited"], false);

        // getWalker
        write_packet(
            &mut stream,
            serde_json::json!({ "to": inspector_actor, "type": "getWalker" }),
        );
        let msg = read_packet(&mut stream);
        let walker = &msg["walker"];
        let walker_actor = walker["actor"].as_str().unwrap().to_string();
        assert_eq!(walker["root"]["nodeType"], 9);

        // querySelector for the flex container
        write_packet(
            &mut stream,
            serde_json::json!({
                "to": walker_actor,
                "type": "querySelector",
                "node": walker["root"]["actor"],
                "selector": "#container",
            }),
        );
        let msg = read_packet(&mut stream);
        let node = &msg["node"];
        assert_eq!(node["nodeName"], "DIV");
        assert_eq!(node["displayType"], "flex");
        let node_actor = node["actor"].as_str().unwrap().to_string();

        // children of the container
        write_packet(
            &mut stream,
            serde_json::json!({ "to": walker_actor, "type": "children", "node": node_actor }),
        );
        let msg = read_packet(&mut stream);
        let children = msg["nodes"].as_array().expect("children nodes");
        assert_eq!(children.len(), 2);
        assert_eq!(children[0]["nodeName"], "SPAN");

        // getPageStyle
        write_packet(
            &mut stream,
            serde_json::json!({ "to": inspector_actor, "type": "getPageStyle" }),
        );
        let msg = read_packet(&mut stream);
        let page_style_actor = msg["pageStyle"]["actor"].as_str().unwrap().to_string();

        // getComputed
        write_packet(
            &mut stream,
            serde_json::json!({ "to": page_style_actor, "type": "getComputed", "node": node_actor }),
        );
        let msg = read_packet(&mut stream);
        let computed = msg["computed"].as_object().expect("computed styles");
        assert_eq!(computed["display"]["value"], "flex");

        // getLayout
        write_packet(
            &mut stream,
            serde_json::json!({ "to": page_style_actor, "type": "getLayout", "node": node_actor }),
        );
        let msg = read_packet(&mut stream);
        assert_eq!(msg["display"], "flex");
        assert!(msg["width"].is_number());

        // getApplied
        write_packet(
            &mut stream,
            serde_json::json!({ "to": page_style_actor, "type": "getApplied", "node": node_actor, "inherited": true }),
        );
        let msg = read_packet(&mut stream);
        let entries = msg["entries"].as_array().expect("applied entries");
        // The div has an inline style attribute, which should be the first entry
        assert!(!entries.is_empty());
        let inline = &entries[0];
        assert_eq!(inline["rule"]["type"], 100);
        assert_eq!(inline["rule"]["declarations"][0]["name"], "display");
        assert_eq!(inline["rule"]["declarations"][0]["value"], "flex");

        // getLayoutInspector + getCurrentFlexbox
        write_packet(
            &mut stream,
            serde_json::json!({ "to": walker_actor, "type": "getLayoutInspector" }),
        );
        let msg = read_packet(&mut stream);
        let layout_actor = msg["actor"]["actor"].as_str().unwrap().to_string();

        write_packet(
            &mut stream,
            serde_json::json!({ "to": layout_actor, "type": "getCurrentFlexbox", "node": node_actor }),
        );
        let msg = read_packet(&mut stream);
        let flexbox = &msg["flexbox"];
        assert!(flexbox.is_object());
        assert_eq!(flexbox["properties"]["flex-direction"], "row");
        let flexbox_actor = flexbox["actor"].as_str().unwrap().to_string();

        write_packet(
            &mut stream,
            serde_json::json!({ "to": flexbox_actor, "type": "getFlexItems" }),
        );
        let msg = read_packet(&mut stream);
        let items = msg["flexitems"].as_array().expect("flex items");
        assert_eq!(items.len(), 2);

        // Highlighting a text node must not crash (it should highlight the
        // nearest element ancestor instead)
        write_packet(
            &mut stream,
            serde_json::json!({
                "to": inspector_actor,
                "type": "getHighlighterByType",
                "typeName": "BoxModelHighlighter",
            }),
        );
        let msg = read_packet(&mut stream);
        let highlighter_actor = msg["highlighter"]["actor"].as_str().unwrap().to_string();

        let span_actor = children[0]["actor"].as_str().unwrap().to_string();
        write_packet(
            &mut stream,
            serde_json::json!({ "to": walker_actor, "type": "children", "node": span_actor }),
        );
        let msg = read_packet(&mut stream);
        let span_children = msg["nodes"].as_array().expect("span children");
        let text_node = &span_children[0];
        assert_eq!(text_node["nodeType"], 3);
        let text_actor = text_node["actor"].as_str().unwrap().to_string();

        write_packet(
            &mut stream,
            serde_json::json!({ "to": highlighter_actor, "type": "show", "node": text_actor }),
        );
        let msg = read_packet(&mut stream);
        assert_eq!(msg["value"], true);
        write_packet(
            &mut stream,
            serde_json::json!({ "to": highlighter_actor, "type": "hide" }),
        );
        let _ = read_packet(&mut stream);

        // A non-atomic inline element that wraps across line boxes has no
        // layout box of its own: getLayout must report the bounding rect of
        // its line-box fragments, and highlighting it must work
        write_packet(
            &mut stream,
            serde_json::json!({
                "to": walker_actor,
                "type": "querySelector",
                "node": walker["root"]["actor"],
                "selector": "#wrapped",
            }),
        );
        let msg = read_packet(&mut stream);
        let wrapped = &msg["node"];
        assert_eq!(wrapped["nodeName"], "SPAN");
        let wrapped_actor = wrapped["actor"].as_str().unwrap().to_string();

        write_packet(
            &mut stream,
            serde_json::json!({ "to": page_style_actor, "type": "getLayout", "node": wrapped_actor }),
        );
        let msg = read_packet(&mut stream);
        assert_eq!(msg["display"], "inline");
        assert!(msg["width"].as_f64().unwrap() > 0.0);
        assert!(msg["height"].as_f64().unwrap() > 0.0);

        write_packet(
            &mut stream,
            serde_json::json!({ "to": highlighter_actor, "type": "show", "node": wrapped_actor }),
        );
        let msg = read_packet(&mut stream);
        assert_eq!(msg["value"], true);
        write_packet(
            &mut stream,
            serde_json::json!({ "to": highlighter_actor, "type": "hide" }),
        );
        let _ = read_packet(&mut stream);

        // Element picker: after `pick`'s empty reply, simulated mouse events
        // should produce pickerNodeHovered/pickerNodePicked events
        write_packet(
            &mut stream,
            serde_json::json!({ "to": walker_actor, "type": "pick", "doFocus": false }),
        );
        let msg = read_packet(&mut stream);
        assert_eq!(msg["from"], walker_actor);
        picker_sender.send(PickerKind::Hovered(10.0, 10.0)).unwrap();
        let msg = read_packet(&mut stream);
        assert_eq!(msg["from"], walker_actor);
        assert_eq!(msg["type"], "pickerNodeHovered");
        let hovered_actor = msg["node"]["node"]["actor"].as_str().unwrap().to_string();

        picker_sender.send(PickerKind::Picked(10.0, 10.0)).unwrap();
        let msg = read_packet(&mut stream);
        assert_eq!(msg["type"], "pickerNodePicked");
        assert_eq!(msg["node"]["node"]["actor"], hovered_actor.as_str());
        assert_eq!(msg["node"]["node"]["nodeType"], 1);

        // cancelPick after picking has stopped is a no-op with an empty reply
        write_packet(
            &mut stream,
            serde_json::json!({ "to": walker_actor, "type": "cancelPick" }),
        );
        let msg = read_packet(&mut stream);
        assert_eq!(msg["from"], walker_actor);
    }
}
