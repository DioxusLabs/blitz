//! Chrome DevTools Protocol server implementation
//!
//! Implements the server side of the [Chrome DevTools Protocol] (the DOM,
//! CSS and Overlay domains) allowing Chrome DevTools' Elements panel to
//! connect to and inspect Blitz documents.
//!
//! Targets are advertised over HTTP at `/json/list` (and `/json/version`),
//! and each document is debuggable over a WebSocket at
//! `/devtools/page/{doc_id}` speaking plain JSON CDP messages.
//!
//! [Chrome DevTools Protocol]: https://chromedevtools.github.io/devtools-protocol/

use serde_json::json;
use std::collections::HashMap;
use std::error::Error;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};
use std::time::Duration;
use tungstenite::Message;
use tungstenite::WebSocket;
use tungstenite::handshake::derive_accept_key;
use tungstenite::protocol::Role;

mod css;
mod dom;
mod session;

use session::Session;

pub(crate) type JsonValue = serde_json::Value;

/// Provides the CDP session with synchronous access to the current set of
/// Blitz documents. Implemented by the embedder (e.g. `BlitzApplication`),
/// which owns the documents.
pub trait DocumentProvider {
    /// The ids of the currently open documents (in tab order)
    fn document_ids(&self) -> Vec<usize>;
    /// Run a callback with mutable access to the document with the given id.
    /// The callback is not run if no such document exists.
    fn with_document(&mut self, id: usize, cb: &mut dyn FnMut(&mut blitz_dom::BaseDocument));
}

/// A waker used to notify the embedder's event loop that there are devtools
/// messages waiting to be processed (via [`CdpServer::process_messages`])
pub trait DevtoolsWaker: Send + Sync {
    /// Wake the event loop
    fn wake(&self);
}

/// An element-picker input event, reported by the embedder while inspect
/// mode is active. Coordinates are in the same viewport-relative CSS pixel
/// space as [`BaseDocument::hit`](blitz_dom::BaseDocument::hit).
#[derive(Debug, Clone, Copy)]
pub enum PickerEvent {
    /// The mouse moved over the document
    Hovered { doc_id: usize, x: f32, y: f32 },
    /// The primary mouse button was pressed over the document
    Picked { doc_id: usize, x: f32, y: f32 },
    /// Picking was cancelled (e.g. the user pressed Escape)
    Canceled { doc_id: usize },
}

pub struct CdpServer {
    connections: HashMap<usize, Connection>,
    waker: Arc<dyn DevtoolsWaker>,
    event_queue: Receiver<CdpEvent>,
    event_sender: Sender<CdpEvent>,
    local_addr: Arc<std::sync::Mutex<Option<std::net::SocketAddr>>>,
}

impl CdpServer {
    pub fn new(waker: Arc<dyn DevtoolsWaker>) -> Self {
        let (sender, receiver) = channel();
        CdpServer {
            connections: HashMap::new(),
            waker,
            event_sender: sender,
            event_queue: receiver,
            local_addr: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// The address the server is listening on (available once the listener
    /// has started). Waits up to `timeout` for the listener to start.
    pub fn wait_for_local_addr(&self, timeout: Duration) -> Option<std::net::SocketAddr> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if let Some(addr) = *self.local_addr.lock().unwrap() {
                return Some(addr);
            }
            if std::time::Instant::now() > deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Start listening for CDP connections on the given address
    /// (e.g. `127.0.0.1:9222`).
    ///
    /// The listener runs on a dedicated background thread (with one thread
    /// per accepted connection). Incoming messages are queued, and the waker
    /// is used to notify the embedder that it should call
    /// [`process_messages`](Self::process_messages).
    pub fn start_listening(&mut self, addr: &str) {
        let sender = self.event_sender.clone();
        let waker = self.waker.clone();
        let msg_cb = Arc::new(move |event: CdpEvent| {
            if sender.send(event).is_ok() {
                waker.wake();
            }
        }) as _;
        let addr = addr.to_string();
        let local_addr = Arc::clone(&self.local_addr);
        std::thread::spawn(move || {
            if let Err(err) = run_cdp_server(addr, msg_cb, local_addr) {
                println!("CDP: server error: {err}");
            }
        });
    }

    /// Process any pending messages from CDP clients. Must be called on
    /// the thread that owns the documents (typically the main/event-loop
    /// thread) whenever the waker fires.
    pub fn process_messages(&mut self, docs: &mut dyn DocumentProvider) {
        while let Ok(event) = self.event_queue.try_recv() {
            self.handle_event(event, docs);
        }
    }

    /// Notify the server of an element-picker input event (called by the
    /// embedder from its input handling path while
    /// [`DevtoolSettings::element_picker`](blitz_traits::devtools::DevtoolSettings)
    /// is set). Emits the corresponding `Overlay` events to any CDP client
    /// currently inspecting that document.
    pub fn notify_picker_event(&mut self, event: PickerEvent, docs: &mut dyn DocumentProvider) {
        for conn in self.connections.values_mut() {
            conn.session
                .handle_picker_event(&mut conn.writer, docs, &event);
        }
    }

    fn handle_event(&mut self, event: CdpEvent, docs: &mut dyn DocumentProvider) {
        match event.data {
            CdpEventData::ConnectionOpened(connection) => {
                self.connections.insert(event.connection_id, connection);
            }
            CdpEventData::ConnectionClosed => {
                if let Some(mut conn) = self.connections.remove(&event.connection_id) {
                    conn.session.close(docs);
                }
            }
            CdpEventData::Command(command) => {
                println!(">> {} {}", command.method, command.params);
                let Some(conn) = self.connections.get_mut(&event.connection_id) else {
                    println!("Error: CDP message from closed connection");
                    return;
                };
                conn.session.handle_command(&mut conn.writer, docs, command);
            }
            CdpEventData::TargetListRequest(reply) => {
                let mut targets = Vec::new();
                for doc_id in docs.document_ids() {
                    docs.with_document(doc_id, &mut |doc| {
                        let title = doc
                            .find_title_node()
                            .map(|node| node.text_content())
                            .unwrap_or_default();
                        targets.push(TargetInfo {
                            id: doc_id,
                            title,
                            url: doc.url().to_string(),
                        });
                    });
                }
                let _ = reply.send(targets);
            }
        }
    }
}

struct Connection {
    writer: MessageWriter,
    session: Session,
}

struct CdpEvent {
    connection_id: usize,
    data: CdpEventData,
}

enum CdpEventData {
    /// A new WebSocket connection was opened
    ConnectionOpened(Connection),
    /// Connection was closed and should be cleaned up
    ConnectionClosed,
    /// A CDP command received from the client
    Command(CdpCommand),
    /// An HTTP `/json/list` request needs the current target list
    TargetListRequest(SyncSender<Vec<TargetInfo>>),
}

/// A CDP command sent by the client: `{id, method, params?, sessionId?}`
pub(crate) struct CdpCommand {
    id: JsonValue,
    method: String,
    params: JsonValue,
    session_id: Option<String>,
}

struct TargetInfo {
    id: usize,
    title: String,
    url: String,
}

pub(crate) struct MessageWriter(Sender<Message>);

impl MessageWriter {
    fn send_json(&mut self, msg: JsonValue) {
        println!("<< {msg}");
        let _ = self.0.send(Message::text(msg.to_string()));
    }

    /// Send a successful reply to a command
    pub(crate) fn reply(&mut self, command: &CdpCommand, result: JsonValue) {
        let mut msg = json!({ "id": command.id, "result": result });
        if let Some(session_id) = &command.session_id {
            msg["sessionId"] = json!(session_id);
        }
        self.send_json(msg);
    }

    /// Send an error reply to a command
    pub(crate) fn reply_err(&mut self, command: &CdpCommand, code: i64, message: &str) {
        let mut msg = json!({ "id": command.id, "error": { "code": code, "message": message } });
        if let Some(session_id) = &command.session_id {
            msg["sessionId"] = json!(session_id);
        }
        self.send_json(msg);
    }

    /// Send a protocol event notification
    pub(crate) fn event(&mut self, method: &str, params: JsonValue) {
        self.send_json(json!({ "method": method, "params": params }));
    }
}

fn run_cdp_server(
    addr: String,
    sender: Arc<dyn Fn(CdpEvent) + Send + Sync>,
    local_addr: Arc<std::sync::Mutex<Option<std::net::SocketAddr>>>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let server = TcpListener::bind(&addr)?;
    let bound_addr = server.local_addr()?;
    *local_addr.lock().unwrap() = Some(bound_addr);
    println!("CDP: listening on: {addr}");

    let mut connection_id_counter: usize = 0;

    loop {
        let (stream, _) = server.accept()?;
        connection_id_counter += 1;
        let connection_id = connection_id_counter;
        let sender = Arc::clone(&sender);
        std::thread::spawn(move || {
            if let Err(err) = handle_connection(stream, connection_id, sender, bound_addr) {
                println!("CDP: connection error: {err}");
            }
        });
    }
}

/// Handle a newly-accepted TCP connection: parse the HTTP request head, then
/// either serve the `/json` discovery endpoints or upgrade to a WebSocket
/// CDP session.
fn handle_connection(
    mut stream: TcpStream,
    connection_id: usize,
    sender: Arc<dyn Fn(CdpEvent) + Send + Sync>,
    bound_addr: std::net::SocketAddr,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    // Read the HTTP request head (up to the blank line)
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    let head_end = loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        head.extend_from_slice(&buf[..n]);
        if let Some(pos) = head.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
        if head.len() > 16 * 1024 {
            return Err("HTTP request head too long".into());
        }
    };

    let head_str = String::from_utf8_lossy(&head[..head_end]).into_owned();
    let mut lines = head_str.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let path = request_line
        .split(' ')
        .nth(1)
        .unwrap_or_default()
        .to_string();
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }

    let is_websocket_upgrade = headers
        .get("upgrade")
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"));

    if !is_websocket_upgrade {
        let body = match path.as_str() {
            "/json" | "/json/list" => {
                // Ask the main thread for the current target list
                let (reply_sender, reply_receiver) = sync_channel(1);
                sender(CdpEvent {
                    connection_id,
                    data: CdpEventData::TargetListRequest(reply_sender),
                });
                let targets = reply_receiver
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap_or_default();

                let list: Vec<JsonValue> = targets
                    .iter()
                    .map(|target| {
                        let ws_path = format!("devtools/page/{}", target.id);
                        json!({
                            "id": target.id.to_string(),
                            "type": "page",
                            "title": target.title,
                            "url": target.url,
                            "description": "",
                            "faviconUrl": "",
                            "webSocketDebuggerUrl": format!("ws://{bound_addr}/{ws_path}"),
                            // devtools_app.html rather than inspector.html: the
                            // latter is the screencast app, which routes inspect
                            // mode and highlighting to its (unsupported here)
                            // screencast view instead of the Overlay domain
                            "devtoolsFrontendUrl": format!(
                                "devtools://devtools/bundled/devtools_app.html?ws={bound_addr}/{ws_path}"
                            ),
                        })
                    })
                    .collect();
                Some(serde_json::to_string_pretty(&list).unwrap())
            }
            "/json/version" => Some(
                serde_json::to_string_pretty(&json!({
                    "Browser": "Blitz",
                    "Protocol-Version": "1.3",
                }))
                .unwrap(),
            ),
            _ => None,
        };

        let response = match body {
            Some(body) => format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=UTF-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            ),
            None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                .to_string(),
        };
        stream.write_all(response.as_bytes())?;
        return Ok(());
    }

    // WebSocket upgrade
    let Some(key) = headers.get("sec-websocket-key") else {
        stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")?;
        return Err("WebSocket upgrade without Sec-WebSocket-Key".into());
    };
    let accept_key = derive_accept_key(key.as_bytes());
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept_key}\r\n\r\n"
    );
    stream.write_all(response.as_bytes())?;

    println!("CDP: new connection (id: {connection_id}, path: {path})");

    // The document id requested via the WebSocket path (`/devtools/page/{id}`)
    let doc_id_hint = path
        .strip_prefix("/devtools/page/")
        .and_then(|id| id.parse::<usize>().ok());

    // A short read timeout lets the single connection thread alternate
    // between blocking reads and draining the outgoing message queue
    stream.set_read_timeout(Some(Duration::from_millis(20)))?;
    let mut ws = WebSocket::from_raw_socket(stream, Role::Server, None);

    let (outgoing_sender, outgoing_receiver) = channel::<Message>();

    sender(CdpEvent {
        connection_id,
        data: CdpEventData::ConnectionOpened(Connection {
            writer: MessageWriter(outgoing_sender),
            session: Session::new(doc_id_hint),
        }),
    });

    let result = connection_loop(&mut ws, connection_id, &sender, outgoing_receiver);

    // Notify the main thread so the connection can be cleaned up
    sender(CdpEvent {
        connection_id,
        data: CdpEventData::ConnectionClosed,
    });

    result
}

/// Service an established WebSocket connection: read incoming CDP commands
/// (forwarding them to the main thread) and write outgoing messages queued
/// by the main thread, until the connection closes.
fn connection_loop(
    ws: &mut WebSocket<TcpStream>,
    connection_id: usize,
    sender: &Arc<dyn Fn(CdpEvent) + Send + Sync>,
    outgoing_receiver: Receiver<Message>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    loop {
        match ws.read() {
            Ok(Message::Text(text)) => {
                let Ok(parsed) = serde_json::from_str::<JsonValue>(&text) else {
                    println!("CDP: invalid JSON message: {text}");
                    continue;
                };
                let Some(method) = parsed.get("method").and_then(|m| m.as_str()) else {
                    println!("CDP: message without method: {text}");
                    continue;
                };
                let command = CdpCommand {
                    id: parsed.get("id").cloned().unwrap_or(JsonValue::Null),
                    method: method.to_string(),
                    params: parsed.get("params").cloned().unwrap_or(json!({})),
                    session_id: parsed
                        .get("sessionId")
                        .and_then(|s| s.as_str())
                        .map(|s| s.to_string()),
                };
                sender(CdpEvent {
                    connection_id,
                    data: CdpEventData::Command(command),
                });
            }
            // Pings are answered automatically by tungstenite
            Ok(Message::Close(_)) => return Ok(()),
            Ok(_) => {}
            // The read timed out: fall through to write any queued messages
            Err(tungstenite::Error::Io(err))
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                return Ok(());
            }
            Err(err) => return Err(err.into()),
        }

        while let Ok(msg) = outgoing_receiver.try_recv() {
            match ws.send(msg) {
                Ok(()) => {}
                Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                    return Ok(());
                }
                Err(err) => return Err(err.into()),
            }
        }
    }
}
