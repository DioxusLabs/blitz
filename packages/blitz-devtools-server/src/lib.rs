//! (Firefox) devtools protocol server implementation

use blitz_traits::shell::EventLoopWaker;
use bytes::{Bytes, BytesMut};
use bytestring::ByteString;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::io::IoSlice;
use std::sync::atomic::{AtomicUsize, Ordering as Ao};
use tokio::io::{AsyncReadExt, AsyncWriteExt, Interest};
// use futures::SinkExt;
// use tokio_stream::StreamExt;
use std::{error::Error, fmt::Display, sync::Arc};
// use string::String as GenericString;
// use tokio::sync::mpsc::{Receiver, Sender, channel};
use std::sync::mpsc::{Receiver, Sender, channel};
use tokio::{
    net::{
        TcpListener, TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
    spawn,
    task::JoinHandle,
};
use tokio_stream::StreamExt;
use tokio_util::codec::{Decoder, Encoder, Framed, FramedRead};

pub struct DevtoolsServer {
    listener: Option<JoinHandle<()>>,
    connections: HashMap<usize, Connection>,
    waker: Arc<dyn EventLoopWaker>,
    event_queue: Receiver<DevtoolsEvent>,
    event_sender: Sender<DevtoolsEvent>,
}

impl DevtoolsServer {
    pub fn new(waker: Arc<dyn EventLoopWaker>) -> Self {
        let (sender, reciever) = channel();
        DevtoolsServer {
            listener: None,
            connections: HashMap::new(),
            waker,
            event_sender: sender,
            event_queue: reciever,
        }
    }

    pub fn start_listening(&mut self, addr: &str) {
        let sender = self.event_sender.clone();
        let waker = self.waker.clone();
        let msg_cb = Arc::new(move |event: DevtoolsEvent| {
            let connection_id = event.connection_id;
            sender.send(event).unwrap();
            waker.wake(connection_id);
        }) as _;
        let listener = tokio::spawn(start_devtools_server_no_err(addr.to_string(), msg_cb));
        self.listener = Some(listener);
    }

    pub fn process_messages(&mut self) {
        while let Ok(event) = self.event_queue.try_recv() {
            self.handle_event(event);
        }
    }

    pub fn handle_event(&mut self, event: DevtoolsEvent) {
        match event.data {
            DevtoolsEventData::ConnectionOpened(connection) => {
                self.connections.insert(event.connection_id, connection);
            }
            DevtoolsEventData::ConnectionClosed => {
                self.connections.remove(&event.connection_id);
            }
            DevtoolsEventData::ClientMessage(msg) => match msg {
                RawMozRdpClientPacket::Json(byte_string) => println!(">> {}", byte_string),
                RawMozRdpClientPacket::Bulk(msg) => {
                    println!(">> bulk to:{} type:{}", msg.to, msg.type_)
                }
            },
            DevtoolsEventData::ServerMessage(msg) => {
                let _ = msg;
            }
        }
    }
}

struct Connection {
    id: usize,
    reader_task: JoinHandle<()>,
    writer: MessageWriter,
}

pub struct DevtoolsEvent {
    connection_id: usize,
    data: DevtoolsEventData,
}

impl DevtoolsEvent {}

enum DevtoolsEventData {
    /// A new connection was opened
    ConnectionOpened(Connection),
    /// Connection was closed and should be cleaned up
    ConnectionClosed,
    /// A message recieved from the client
    ClientMessage(RawMozRdpClientPacket),
    /// A message from Blitz to send to the client
    ServerMessage(MozRdpServerPacket),
}

pub(crate) struct MessageWriter(OwnedWriteHalf);

impl MessageWriter {
    async fn write_msg(&mut self, msg: &str) {
        self.0.writable().await.unwrap();

        println!("<< {msg}");

        let len = msg.len();
        let len_s = format!("{len}:");
        self.0
            .write_vectored(&[IoSlice::new(len_s.as_bytes()), IoSlice::new(msg.as_bytes())])
            .await
            .unwrap();
    }
}

// type ClientMessageCallback = Arc<dyn Fn(usize, RawMozRdpClientPacket) + Send + Sync>;

async fn start_devtools_server_no_err(
    addr: String,
    msg_cb: Arc<dyn Fn(DevtoolsEvent) + Send + Sync>,
) {
    start_devtools_server(addr, msg_cb).await.unwrap();
}

async fn start_devtools_server(
    addr: String,
    sender: Arc<dyn Fn(DevtoolsEvent) + Send + Sync>,
) -> Result<Sender<MozRdpServerPacket>, Box<dyn Error + Send + Sync>> {
    // Parse the arguments, bind the TCP socket we'll be listening to, spin up
    // our worker threads, and start shipping sockets to those worker threads.
    let server = TcpListener::bind(&addr).await?;
    println!("Devtools: listening on: {addr}");

    let mut connection_id_counter: usize = 0;

    // let (write_sender, writer_recv) = channel::<(usize, MozRdpServerPacket)>(100);

    loop {
        let (stream, _) = server.accept().await?;
        let (reader, writer) = stream.into_split();

        connection_id_counter += 1;
        let connection_id = connection_id_counter;

        println!("Devtools: new connection (id: {})", connection_id);

        // Spawn stream reader task
        let task = tokio::spawn({
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
                            println!("Err parsing devtools packet {:?}", e);
                        }
                    }
                }

                // DEBUG (non-framed IO)
                //
                // let mut buf = BytesMut::with_capacity(64 * 1024);
                // loop {
                //     reader.ready(Interest::READABLE).await.unwrap();
                //     reader.read_buf(&mut buf).await.unwrap();
                //     // println!("{}", buf.len());
                // }
            }
        });

        let mut writer = MessageWriter(writer);

        // Send inital message
        writer.write_msg(r#"{ "from": "root", "applicationType": "browser", "traits": { "sources": false, "highlightable": true, "customHighlighters": true, "networkMonitor": false } }"#).await;

        // Send event with new connection
        sender(DevtoolsEvent {
            connection_id,
            data: DevtoolsEventData::ConnectionOpened(Connection {
                id: connection_id,
                reader_task: task,
                writer,
            }),
        });

        // // Spawn stream writer task
        // tokio::spawn(async move {
        //     while let Some(msg) = writer.recv().await {
        //         match writer.try_write(&serde_json::to_vec(&msg).unwrap()) {
        //             Ok(request) => continue,
        //             Err(e) => {
        //                 println!("Err writing devtools packet {:?}", e);
        //             }
        //         }
        //     }
        // });
    }
}

// async fn stream_reader(
//     reader: OwnedReadHalf,
//     msg_cb: ClientMessageCallback,
// ) -> Result<(), Box<dyn Error>> {
//     let mut framed_reader = FramedRead::new(reader, MozRdpStreamTransport::default());
//     while let Some(request) = framed_reader.next().await {
//         match request {
//             Ok(request) => {
//                 let response = respond(request).await?;
//                 transport.send(response).await?;
//             }
//             Err(e) => return Err(e.into()),
//         }
//     }

//     Ok(())
// }

/// A MozRdp message sent from the server
#[derive(Serialize)]
struct MozRdpServerPacket {
    from: String,
    msg: serde_json::Value,
}

// async fn stream_writer(
//     writer: OwnedWriteHalf,
//     channel: tokio::sync::mpsc::Receiver<MozRdpServerPacket>,
// ) -> Result<(), Box<dyn Error>> {
//     // let mut framed_reader = FramedRead::new(reader, MozRdpStreamTransport::default());
//     while let Some(msg) = channel.recv().await {
//         writer.try_write(msg)
//     }

//     Ok(())
// }

#[derive(Default)]
struct MozRdpStreamTransport {
    header: Option<MozRdpHeader>,
}

#[derive(Debug)]
enum PacketDecodeErr {
    HeaderTooLong,
    InvalidHeader,
    InvalidUtf8,
    IoError(std::io::Error),
}

impl Display for PacketDecodeErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PacketDecodeErr::HeaderTooLong => write!(f, "Header too long"),
            PacketDecodeErr::InvalidHeader => write!(f, "InvalidHeader"),
            PacketDecodeErr::InvalidUtf8 => write!(f, "InvalidUTF8"),
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
    type Item = RawMozRdpClientPacket;

    type Error = PacketDecodeErr;

    fn decode_eof(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // println!(
        //     "Decode EOF ({}): {}",
        //     src.len(),
        //     str::from_utf8(&src).unwrap()
        // );
        Ok(None)
    }

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // println!("Decode ({}): {}", src.len(), str::from_utf8(&src).unwrap());
        if src.len() == 0 {
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
                let data = ByteString::try_from(data).map_err(|_| PacketDecodeErr::InvalidUtf8)?;
                Ok(Some(RawMozRdpClientPacket::Json(data)))
            }
            MozRdpPacketKind::Bulk { to, type_ } => {
                let data = src.split_to(header.expected_data_length).freeze();
                Ok(Some(RawMozRdpClientPacket::Bulk(MozRdpClientPacket {
                    to,
                    type_,
                    data,
                })))
            }
        }
    }
}

#[derive(Debug)]
struct MozRdpPacketErr;

impl Display for MozRdpPacketErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MozRdpPacketErr")
    }
}

impl core::error::Error for MozRdpPacketErr {}

#[derive(Clone)]
enum MozRdpPacketKind {
    Json,
    Bulk { to: String, type_: String },
}

#[derive(Clone)]
struct MozRdpHeader {
    /// The length of a successfully parsed header
    /// (up to and *including* the terminating semi-colon)
    header_length: usize,
    /// The length of the data indicated from the header.
    expected_data_length: usize,
    /// The kind of packet (JSON or Bulk)
    header_kind: MozRdpPacketKind,
}

impl MozRdpHeader {
    fn try_parse(input: &[u8]) -> Result<Option<Self>, ()> {
        // Try to parse JSON packet header
        if input.iter().all(|c| *c >= b'0' && *c <= b'9') {
            return Ok(Some(Self {
                header_length: input.len() + 1,
                expected_data_length: str::from_utf8(input).unwrap().parse().unwrap(),
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
                header_length: input.len() + 1,
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
pub enum RawMozRdpClientPacket {
    Json(ByteString),
    Bulk(MozRdpClientPacket<Bytes>),
}

/// A MozRdp message sent from the client
#[derive(Serialize, Deserialize)]
pub struct MozRdpClientPacket<T> {
    pub to: String,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(flatten)]
    pub data: T,
}
