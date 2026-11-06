//! Chrome DevTools Protocol server implementation
//!
//! Implements the server side of the [Chrome DevTools Protocol] (the DOM,
//! CSS and Overlay domains) allowing Chrome DevTools' Elements panel to
//! connect to and inspect Blitz documents.
//!
//! Targets are advertised over HTTP at `/json/list` (and `/json/version`),
//! and each target is debuggable over a WebSocket at
//! `/devtools/page/{target_id}` speaking plain JSON CDP messages. A top-level
//! document's target id is its document id. A sub-document's target id is
//! that of the element hosting it (`{host_doc_id}.{host_node_id}`), so the
//! target follows the host as it navigates to new documents (e.g. a browser
//! tab or an iframe).
//!
//! [Chrome DevTools Protocol]: https://chromedevtools.github.io/devtools-protocol/

use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel};
use std::time::Duration;

use blitz_traits::node_id::NodeId;
use tungstenite::Message;

mod css;
mod devtools_provider;
pub mod documents;
mod dom;
mod session;
mod style_sheets;
mod transport;

use devtools_provider::MutationLog;
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
    mutation_log: Arc<MutationLog>,
}

impl CdpServer {
    pub fn new(waker: Arc<dyn DevtoolsWaker>) -> Self {
        let (sender, receiver) = channel();
        CdpServer {
            connections: HashMap::new(),
            mutation_log: Arc::new(MutationLog::new(waker.clone())),
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

    /// Process any pending messages from CDP clients and send events for
    /// any DOM mutations made to inspected documents. Must be called on the
    /// thread that owns the documents (typically the main/event-loop thread)
    /// whenever the waker fires. The waker also fires after the first
    /// mutation to an inspected document since the last call.
    pub fn process_messages(&mut self, docs: &mut dyn DocumentProvider) {
        // Move sessions inspecting a dropped sub-document on to the
        // document that replaced it (if any)
        if self.mutation_log.take_documents_dropped() {
            for conn in self.connections.values_mut() {
                conn.session.rebind(&mut conn.writer, docs);
            }
        }
        while let Ok(event) = self.event_queue.try_recv() {
            self.handle_event(event, docs);
        }
        self.flush_mutations(docs);
        for conn in self.connections.values_mut() {
            conn.session.sync_style_sheets(&mut conn.writer, docs);
        }
    }

    /// Send the events for all buffered DOM mutations to the sessions
    /// inspecting the mutated documents
    fn flush_mutations(&mut self, docs: &mut dyn DocumentProvider) {
        let records = self.mutation_log.take();
        if records.is_empty() {
            return;
        }
        for conn in self.connections.values_mut() {
            let Some(doc_id) = conn.session.doc_id(docs) else {
                continue;
            };
            let doc_records = records
                .iter()
                .filter(move |(id, _)| *id == doc_id)
                .map(|(_, record)| record);
            if doc_records.clone().next().is_none() {
                continue;
            }
            with_document(docs, doc_id, |doc| {
                conn.session
                    .emit_mutations(&mut conn.writer, doc, doc_records.clone());
            });
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
            CdpEventData::ConnectionOpened(mut connection) => {
                connection.session.rebind(&mut connection.writer, docs);
                self.connections.insert(event.connection_id, connection);
            }
            CdpEventData::ConnectionClosed => {
                if let Some(mut conn) = self.connections.remove(&event.connection_id) {
                    conn.session.close(docs);
                    // Stop recording mutations once no session inspects the document
                    if let Some(doc_id) = conn.session.doc_id(docs)
                        && !self
                            .connections
                            .values()
                            .any(|other| other.session.doc_id(docs) == Some(doc_id))
                    {
                        with_document(docs, doc_id, |doc| doc.set_devtools_provider(None));
                    }
                }
            }
            CdpEventData::Command(command) => {
                tracing::debug!("CDP: >> {}", command.method);
                tracing::trace!("CDP: >> {} {}", command.method, command.params);
                if !self.connections.contains_key(&event.connection_id) {
                    tracing::warn!("CDP: message from closed connection");
                    return;
                }
                // Events for mutations made before the command describe the
                // tree the command sees, and those for mutations the command
                // makes are sent before its reply (as Chrome does)
                self.flush_mutations(docs);
                let conn = self.connections.get_mut(&event.connection_id).unwrap();
                // The host may have navigated since the session was last
                // bound without the document being dropped while recorded
                // (e.g. before the session's first `DOM.getDocument`)
                conn.session.rebind(&mut conn.writer, docs);
                let result = conn.session.dispatch(&mut conn.writer, docs, &command);
                // Record mutations to the document from now on: the frontend
                // has a tree to apply them to
                if command.method == "DOM.getDocument"
                    && result.is_ok()
                    && let Some(doc_id) = conn.session.doc_id(docs)
                {
                    let recorder = self.mutation_log.recorder(doc_id);
                    with_document(docs, doc_id, |doc| {
                        doc.set_devtools_provider(Some(recorder.clone()))
                    });
                }
                self.flush_mutations(docs);
                let conn = self.connections.get_mut(&event.connection_id).unwrap();
                match result {
                    Ok(result) => conn.writer.reply(&command, result),
                    Err(err) => conn.writer.reply_err(&command, err.code, &err.message),
                }
            }
            CdpEventData::TargetListRequest(reply) => {
                let mut targets = Vec::new();
                for (target_id, doc_id) in list_targets(docs) {
                    docs.with_document(doc_id, &mut |doc| {
                        let title = match doc.devtools_title() {
                            Some(title) => title.to_string(),
                            None => doc
                                .find_title_node()
                                .map(|node| node.text_content())
                                .unwrap_or_default(),
                        };
                        targets.push(TargetInfo {
                            id: target_id,
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

/// The id of a debuggable target
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetId {
    /// A top-level document, by document id
    Document(usize),
    /// Whichever document is hosted by an element (e.g. a browser tab's
    /// web view or an iframe), which stays the same target as the element
    /// navigates to new documents
    SubDocument(SubDocumentHost),
}

/// An element hosting a sub-document
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SubDocumentHost {
    pub(crate) doc_id: usize,
    pub(crate) node_id: NodeId,
}

impl TargetId {
    /// Parse a target id as formatted by its `Display` impl
    pub(crate) fn parse(id: &str) -> Option<Self> {
        match id.split_once('.') {
            Some((doc_id, node_id)) => Some(TargetId::SubDocument(SubDocumentHost {
                doc_id: doc_id.parse().ok()?,
                node_id: NodeId::from_u64(node_id.parse().ok()?),
            })),
            None => Some(TargetId::Document(id.parse().ok()?)),
        }
    }

    /// The id of the document the target currently shows (if it exists)
    pub(crate) fn resolve(&self, docs: &mut dyn DocumentProvider) -> Option<usize> {
        match self {
            TargetId::Document(doc_id) => docs.document_ids().contains(doc_id).then_some(*doc_id),
            TargetId::SubDocument(host) => {
                let mut sub_doc_id = None;
                with_document(docs, host.doc_id, |doc| {
                    sub_doc_id = doc
                        .get_node(host.node_id)
                        .and_then(|node| node.subdoc())
                        .map(|sub_doc| sub_doc.id());
                });
                sub_doc_id
            }
        }
    }
}

impl std::fmt::Display for TargetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TargetId::Document(doc_id) => write!(f, "{doc_id}"),
            TargetId::SubDocument(host) => {
                write!(f, "{}.{}", host.doc_id, host.node_id.as_u64())
            }
        }
    }
}

/// The current targets (in document order), each with the id of the
/// document it shows
fn list_targets(docs: &mut dyn DocumentProvider) -> Vec<(TargetId, usize)> {
    let doc_ids = docs.document_ids();
    let mut hosts = HashMap::new();
    for &doc_id in &doc_ids {
        with_document(docs, doc_id, |doc| {
            for node_id in doc.sub_document_node_ids() {
                if let Some(sub_doc) = doc.get_node(node_id).and_then(|node| node.subdoc()) {
                    hosts.insert(sub_doc.id(), SubDocumentHost { doc_id, node_id });
                }
            }
        });
    }
    doc_ids
        .into_iter()
        .map(|doc_id| match hosts.get(&doc_id) {
            Some(host) => (TargetId::SubDocument(*host), doc_id),
            None => (TargetId::Document(doc_id), doc_id),
        })
        .collect()
}

pub(crate) struct TargetInfo {
    pub(crate) id: TargetId,
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

/// Run a callback with access to the document with the given id (if it exists)
fn with_document(
    docs: &mut dyn DocumentProvider,
    doc_id: usize,
    cb: impl FnOnce(&mut blitz_dom::BaseDocument),
) {
    let mut cb = Some(cb);
    docs.with_document(doc_id, &mut |doc| {
        if let Some(cb) = cb.take() {
            cb(doc);
        }
    });
}
