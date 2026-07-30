//! (Firefox) devtools protocol server implementation
//!
//! Implements the server side of the [Firefox Remote Debugging Protocol]
//! allowing Firefox's devtools (in particular: the DOM, style and layout
//! inspectors) to connect to and inspect Blitz documents.
//!
//! [Firefox Remote Debugging Protocol]: https://firefox-source-docs.mozilla.org/devtools/backend/protocol.html

use actors::{Actor, ActorId, ActorMessageErr};
use blitz_dom::BaseDocument;
use bytes::{Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::io::IoSlice;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::{error::Error, fmt::Display, sync::Arc};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::Sender as TokioSender;
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_stream::StreamExt;
use tokio_util::codec::{Decoder, FramedRead};

mod actors;

pub(crate) type JsonValue = serde_json::Value;

/// Provides devtools actors with synchronous access to the current set of
/// Blitz documents. Implemented by the embedder (e.g. `BlitzApplication`),
/// which owns the documents.
pub trait DocumentProvider {
    /// The ids of the currently open documents (in tab order)
    fn document_ids(&self) -> Vec<usize>;
    /// Run a callback with mutable access to the document with the given id.
    /// The callback is not run if no such document exists.
    fn with_document(&mut self, id: usize, cb: &mut dyn FnMut(&mut BaseDocument));
}

/// A waker used to notify the embedder's event loop that there are devtools
/// messages waiting to be processed (via [`DevtoolsServer::process_messages`])
pub trait DevtoolsWaker: Send + Sync {
    /// Wake the event loop
    fn wake(&self);
}

pub struct DevtoolsServer {
    runtime: Option<tokio::runtime::Runtime>,
    listener: Option<JoinHandle<()>>,
    connections: HashMap<usize, Connection>,
    waker: Arc<dyn DevtoolsWaker>,
    event_queue: Receiver<DevtoolsEvent>,
    event_sender: Sender<DevtoolsEvent>,
    local_addr: Arc<std::sync::Mutex<Option<std::net::SocketAddr>>>,
}

impl DevtoolsServer {
    pub fn new(waker: Arc<dyn DevtoolsWaker>) -> Self {
        let (sender, receiver) = channel();
        DevtoolsServer {
            runtime: None,
            listener: None,
            connections: HashMap::new(),
            waker,
            event_sender: sender,
            event_queue: receiver,
            local_addr: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// The address the server is listening on (available once the listener
    /// has started). Waits up to `timeout` for the listener to start.
    pub fn wait_for_local_addr(
        &self,
        timeout: std::time::Duration,
    ) -> Option<std::net::SocketAddr> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if let Some(addr) = *self.local_addr.lock().unwrap() {
                return Some(addr);
            }
            if std::time::Instant::now() > deadline {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// Start listening for devtools connections on the given address
    /// (e.g. `127.0.0.1:6000`).
    ///
    /// The listener runs on a dedicated background thread. Incoming messages
    /// are queued, and the waker is used to notify the embedder that it
    /// should call [`process_messages`](Self::process_messages).
    pub fn start_listening(&mut self, addr: &str) {
        let sender = self.event_sender.clone();
        let waker = self.waker.clone();
        let msg_cb = Arc::new(move |event: DevtoolsEvent| {
            if sender.send(event).is_ok() {
                waker.wake();
            }
        }) as _;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_io()
            .build()
            .expect("failed to build devtools tokio runtime");
        let listener = runtime.spawn(start_devtools_server_no_err(
            addr.to_string(),
            msg_cb,
            Arc::clone(&self.local_addr),
        ));
        self.runtime = Some(runtime);
        self.listener = Some(listener);
    }

    /// Process any pending messages from devtools clients. Must be called on
    /// the thread that owns the documents (typically the main/event-loop
    /// thread) whenever the waker fires.
    pub fn process_messages(&mut self, docs: &mut dyn DocumentProvider) {
        while let Ok(event) = self.event_queue.try_recv() {
            self.handle_event(event, docs);
        }
    }

    fn handle_event(&mut self, event: DevtoolsEvent, docs: &mut dyn DocumentProvider) {
        match event.data {
            DevtoolsEventData::ConnectionOpened(connection) => {
                self.connections.insert(event.connection_id, connection);
            }
            DevtoolsEventData::ConnectionClosed => {
                if let Some(conn) = self.connections.remove(&event.connection_id) {
                    conn.reader_task.abort();
                    conn.writer_task.abort();
                }
            }
            DevtoolsEventData::ClientMessage(msg) => {
                msg.debug_log();

                let Some(conn) = self.connections.get_mut(&event.connection_id) else {
                    println!("Error: Devtools message from closed connection");
                    return;
                };

                conn.handle_message(msg, docs);
            }
        }
    }
}

impl Drop for DevtoolsServer {
    fn drop(&mut self) {
        if let Some(listener) = self.listener.take() {
            listener.abort();
        }
        for conn in self.connections.values() {
            conn.reader_task.abort();
            conn.writer_task.abort();
        }
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

struct Connection {
    #[allow(dead_code)]
    id: usize,
    reader_task: JoinHandle<()>,
    writer_task: JoinHandle<()>,
    writer: MessageWriter,
    actors: HashMap<ActorId, Box<dyn Actor>>,
}

pub struct DevtoolsEvent {
    connection_id: usize,
    data: DevtoolsEventData,
}

enum DevtoolsEventData {
    /// A new connection was opened
    ConnectionOpened(Connection),
    /// Connection was closed and should be cleaned up
    ConnectionClosed,
    /// A message received from the client
    ClientMessage(GenericClientMessage),
}

pub(crate) struct MessageWriter(TokioSender<ServerMessage<JsonValue>>);

impl MessageWriter {
    fn write_msg(&mut self, from: String, data: JsonValue) {
        let _ = self.0.try_send(ServerMessage { from, data });
    }
    fn write_err(&mut self, from: String, err: ActorMessageErr) {
        let data = json!({ "error": err.as_str(), "message": err.message() });
        let _ = self.0.try_send(ServerMessage { from, data });
    }
}

async fn start_devtools_server_no_err(
    addr: String,
    msg_cb: Arc<dyn Fn(DevtoolsEvent) + Send + Sync>,
    local_addr: Arc<std::sync::Mutex<Option<std::net::SocketAddr>>>,
) {
    if let Err(err) = start_devtools_server(addr, msg_cb, local_addr).await {
        println!("Devtools: server error: {err}");
    }
}

async fn start_devtools_server(
    addr: String,
    sender: Arc<dyn Fn(DevtoolsEvent) + Send + Sync>,
    local_addr: Arc<std::sync::Mutex<Option<std::net::SocketAddr>>>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let server = TcpListener::bind(&addr).await?;
    if let Ok(addr) = server.local_addr() {
        *local_addr.lock().unwrap() = Some(addr);
    }
    println!("Devtools: listening on: {addr}");

    let mut connection_id_counter: usize = 0;

    loop {
        let (stream, _) = server.accept().await?;
        let (reader, mut writer) = stream.into_split();

        connection_id_counter += 1;
        let connection_id = connection_id_counter;

        println!("Devtools: new connection (id: {connection_id})");

        // Spawn stream reader task
        let reader_task = tokio::spawn({
            let sender = Arc::clone(&sender);
            async move {
                let mut framed_reader = FramedRead::new(reader, MozRdpStreamTransport::default());
                while let Some(msg) = framed_reader.next().await {
                    match msg {
                        Ok(msg) => {
                            sender(DevtoolsEvent {
                                connection_id,
                                data: DevtoolsEventData::ClientMessage(msg),
                            });
                        }
                        Err(e) => {
                            println!("Err parsing devtools packet {e:?}");
                            break;
                        }
                    }
                }

                // Stream ended: notify the main thread so the connection can
                // be cleaned up
                sender(DevtoolsEvent {
                    connection_id,
                    data: DevtoolsEventData::ConnectionClosed,
                });
            }
        });

        // Spawn stream writer task
        let (outgoing_sender, mut outgoing_receiver) =
            tokio::sync::mpsc::channel::<ServerMessage<JsonValue>>(100);
        let writer_task = tokio::spawn(async move {
            while let Some(msg) = outgoing_receiver.recv().await {
                if msg.data.get("error").is_some() {
                    println!("<< FROM:{} ERROR {}", msg.from, msg.data);
                } else {
                    println!("<< FROM:{} {}", msg.from, msg.data);
                }

                let encoded = serde_json::to_string(&msg).unwrap();
                let len = encoded.len();
                let len_s = format!("{len}:");
                let write_result = writer
                    .write_vectored(&[
                        IoSlice::new(len_s.as_bytes()),
                        IoSlice::new(encoded.as_bytes()),
                    ])
                    .await;
                if write_result.is_err() {
                    break;
                }
            }
        });

        // Send initial message
        let mut writer = MessageWriter(outgoing_sender);
        writer.write_msg(String::from("root"), json!({
            "applicationType": "browser",
            "traits": { "sources": false, "highlightable": true, "customHighlighters": true, "networkMonitor": false }
        }));

        let mut connection = Connection {
            id: connection_id,
            reader_task,
            writer_task,
            writer,
            actors: HashMap::new(),
        };
        connection.init();

        // Send event with new connection
        sender(DevtoolsEvent {
            connection_id,
            data: DevtoolsEventData::ConnectionOpened(connection),
        });
    }
}

#[derive(Default)]
struct MozRdpStreamTransport {
    header: Option<MozRdpHeader>,
}

#[derive(Debug)]
enum PacketDecodeErr {
    HeaderTooLong,
    InvalidHeader,
    #[allow(dead_code)]
    InvalidUtf8,
    InvalidJson,
    IoError(std::io::Error),
}

impl Display for PacketDecodeErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PacketDecodeErr::HeaderTooLong => write!(f, "Header too long"),
            PacketDecodeErr::InvalidHeader => write!(f, "InvalidHeader"),
            PacketDecodeErr::InvalidUtf8 => write!(f, "InvalidUTF8"),
            PacketDecodeErr::InvalidJson => write!(f, "InvalidJson"),
            PacketDecodeErr::IoError(err) => err.fmt(f),
        }
    }
}

impl Error for PacketDecodeErr {}

impl From<std::io::Error> for PacketDecodeErr {
    fn from(value: std::io::Error) -> Self {
        PacketDecodeErr::IoError(value)
    }
}

impl Decoder for MozRdpStreamTransport {
    type Item = GenericClientMessage;

    type Error = PacketDecodeErr;

    fn decode_eof(&mut self, _src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        Ok(None)
    }

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.is_empty() {
            return Ok(None);
        }

        let header = match &self.header {
            Some(header) => header,
            None => {
                let Some(position) = src.iter().position(|b| *b == b':') else {
                    if src.len() > 1000 {
                        // Input excessively long: assuming invalid packet
                        return Err(PacketDecodeErr::HeaderTooLong);
                    } else {
                        // Incomplete header
                        return Ok(None);
                    }
                };
                let header_input = &src[0..position];

                match MozRdpHeader::try_parse(header_input)
                    .map_err(|_| PacketDecodeErr::InvalidHeader)?
                {
                    Some(header) => {
                        let _ = src.split_to(position + 1);
                        self.header = Some(header);
                        self.header.as_ref().unwrap()
                    }
                    None => return Ok(None),
                }
            }
        };

        if src.len() < header.expected_data_length {
            return Ok(None);
        }

        let header = self.header.take().unwrap();
        match header.header_kind {
            MozRdpPacketKind::Json => {
                let data = src.split_to(header.expected_data_length).freeze();
                let msg: ClientMessage<JsonValue> =
                    serde_json::from_slice(&data).map_err(|_| PacketDecodeErr::InvalidJson)?;
                Ok(Some(msg.into()))
            }
            MozRdpPacketKind::Bulk { to, type_ } => {
                let data = src.split_to(header.expected_data_length).freeze();
                Ok(Some(ClientMessage { to, type_, data }.into()))
            }
        }
    }
}

#[derive(Clone)]
enum MozRdpPacketKind {
    Json,
    Bulk { to: String, type_: String },
}

#[derive(Clone)]
struct MozRdpHeader {
    /// The length of the data indicated from the header.
    expected_data_length: usize,
    /// The kind of packet (JSON or Bulk)
    header_kind: MozRdpPacketKind,
}

impl MozRdpHeader {
    fn try_parse(input: &[u8]) -> Result<Option<Self>, ()> {
        // Try to parse JSON packet header
        if input.iter().all(|c| c.is_ascii_digit()) {
            return Ok(Some(Self {
                expected_data_length: str::from_utf8(input)
                    .map_err(|_| ())?
                    .parse()
                    .map_err(|_| ())?,
                header_kind: MozRdpPacketKind::Json,
            }));
        }

        // Try to parse Bulk packet header
        if input.starts_with(b"bulk ") {
            let s = str::from_utf8(&input[5..]).map_err(|_| ())?;
            let mut parts = s.splitn(3, ' ');
            let to = parts.next().ok_or(())?;
            let type_ = parts.next().ok_or(())?;
            let length_str = parts.next().ok_or(())?;
            let length = length_str.parse().map_err(|_| ())?;

            return Ok(Some(Self {
                expected_data_length: length,
                header_kind: MozRdpPacketKind::Bulk {
                    to: to.to_string(),
                    type_: type_.to_string(),
                },
            }));
        }

        // Return error
        Err(())
    }
}

/// A Mozilla Remote Debugging Protocol packet with unparsed data field
pub enum GenericClientData {
    Json(JsonValue),
    Bulk(Bytes),
}

pub(crate) type GenericClientMessage = ClientMessage<GenericClientData>;
pub(crate) type JsonClientMessage = ClientMessage<JsonValue>;
pub(crate) type BulkClientMessage = ClientMessage<Bytes>;

impl From<JsonClientMessage> for GenericClientMessage {
    fn from(value: JsonClientMessage) -> Self {
        ClientMessage {
            to: value.to,
            type_: value.type_,
            data: GenericClientData::Json(value.data),
        }
    }
}

impl From<BulkClientMessage> for GenericClientMessage {
    fn from(value: BulkClientMessage) -> Self {
        ClientMessage {
            to: value.to,
            type_: value.type_,
            data: GenericClientData::Bulk(value.data),
        }
    }
}

impl GenericClientData {
    pub(crate) fn json(&self) -> Result<&JsonValue, ActorMessageErr> {
        match self {
            GenericClientData::Json(value) => Ok(value),
            GenericClientData::Bulk(_) => Err(ActorMessageErr::BadParameterType),
        }
    }
}

impl GenericClientMessage {
    pub(crate) fn debug_log(&self) {
        match &self.data {
            GenericClientData::Json(json) => println!(
                ">>   TO:{} {} {}",
                self.to,
                self.type_,
                serde_json::to_string(&json).unwrap()
            ),
            GenericClientData::Bulk(bytes) => {
                println!(
                    ">> bulk to:{} type:{} ({} bytes)",
                    self.to,
                    self.type_,
                    bytes.len()
                )
            }
        }
    }
}

/// A MozRdp message sent from the client
#[derive(Serialize, Deserialize)]
pub struct ClientMessage<T> {
    pub to: String,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(flatten)]
    pub data: T,
}

/// A MozRdp message sent from the server
#[derive(Serialize, Deserialize)]
pub struct ServerMessage<T> {
    pub from: String,
    #[serde(flatten)]
    pub data: T,
}
