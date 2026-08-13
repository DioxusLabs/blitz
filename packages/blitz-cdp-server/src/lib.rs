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
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel};
use std::time::Duration;
use tungstenite::Message;

mod css;
pub mod documents;
mod dom;
mod session;
mod transport;

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
            if let Err(err) = transport::run_cdp_server(addr, msg_cb, local_addr) {
                tracing::warn!("CDP: server error: {err}");
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
                tracing::debug!("CDP: >> {}", command.method);
                tracing::trace!("CDP: >> {} {}", command.method, command.params);
                let Some(conn) = self.connections.get_mut(&event.connection_id) else {
                    tracing::warn!("CDP: message from closed connection");
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

pub(crate) struct Connection {
    pub(crate) writer: MessageWriter,
    pub(crate) session: Session,
}

pub(crate) struct CdpEvent {
    pub(crate) connection_id: usize,
    pub(crate) data: CdpEventData,
}

pub(crate) enum CdpEventData {
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
    pub(crate) id: JsonValue,
    pub(crate) method: String,
    pub(crate) params: JsonValue,
    pub(crate) session_id: Option<String>,
}

pub(crate) struct TargetInfo {
    pub(crate) id: usize,
    pub(crate) title: String,
    pub(crate) url: String,
}

pub(crate) struct MessageWriter(pub(crate) Sender<Message>);

impl MessageWriter {
    fn send_json(&mut self, msg: JsonValue) {
        tracing::trace!("CDP: << {msg}");
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
