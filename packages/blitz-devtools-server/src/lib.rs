//! (Firefox) devtools protocol server implementation

use bytes::{Bytes, BytesMut};
use bytestring::ByteString;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering as Ao};
// use futures::SinkExt;
// use tokio_stream::StreamExt;
use std::{error::Error, fmt::Display, sync::Arc};
// use string::String as GenericString;
use tokio::sync::mpsc::{Receiver, Sender, channel};
use tokio::{
    net::{
        TcpListener, TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
    spawn,
};
use tokio_stream::StreamExt;
use tokio_util::codec::{Decoder, Encoder, Framed, FramedRead};

pub struct DevtoolsServer;

pub enum DevtoolsEvent {
    NewConnection(TcpStream),
    ClientMessage(usize, RawMozRdpClientPacket),
    ServerMessage(usize, MozRdpServerPacket),
}

type ClientMessageCallback = Arc<dyn Fn(usize, RawMozRdpClientPacket) + Send + Sync>;

async fn start_devtools_server(
    msg_cb: ClientMessageCallback,
) -> Result<Sender<MozRdpServerPacket>, Box<dyn Error>> {
    // Parse the arguments, bind the TCP socket we'll be listening to, spin up
    // our worker threads, and start shipping sockets to those worker threads.
    let addr = "127.0.0.1:6080";
    let server = TcpListener::bind(&addr).await?;
    println!("Listening on: {addr}");

    static CONNECTION_ID_COUNTER: AtomicUsize = AtomicUsize::new(0);

    let (write_sender, writer_recv) = channel::<(usize, MozRdpServerPacket)>(100);

    loop {
        let (stream, _) = server.accept().await?;
        let (reader, writer) = stream.into_split();

        let connection_id = CONNECTION_ID_COUNTER.fetch_add(1, Ao::Relaxed);
        let msg_cb = Arc::clone(&msg_cb);

        // Spawn stream reader task
        tokio::spawn(async move {
            let mut framed_reader = FramedRead::new(reader, MozRdpStreamTransport::default());
            while let Some(msg) = framed_reader.next().await {
                match msg {
                    Ok(msg) => msg_cb(connection_id, msg),
                    Err(e) => {
                        println!("Err parsing devtools packet {:?}", e);
                    }
                }
            }
        });

        // Spawn stream writer task
        tokio::spawn(async move {
            while let Some(msg) = writer_recv.recv().await {
                match writer.try_write(&serde_json::to_vec(&msg).unwrap()) {
                    Ok(request) => continue,
                    Err(e) => {
                        println!("Err writing devtools packet {:?}", e);
                    }
                }
            }
        });
    }
}

async fn stream_reader(
    reader: OwnedReadHalf,
    msg_cb: ClientMessageCallback,
) -> Result<(), Box<dyn Error>> {
    let mut framed_reader = FramedRead::new(reader, MozRdpStreamTransport::default());
    while let Some(request) = framed_reader.next().await {
        match request {
            Ok(request) => {
                let response = respond(request).await?;
                transport.send(response).await?;
            }
            Err(e) => return Err(e.into()),
        }
    }

    Ok(())
}

/// A MozRdp message sent from the server
#[derive(Serialize)]
struct MozRdpServerPacket {
    from: String,
    msg: serde_json::Value,
}

async fn stream_writer(
    writer: OwnedWriteHalf,
    channel: tokio::sync::mpsc::Receiver<MozRdpServerPacket>,
) -> Result<(), Box<dyn Error>> {
    // let mut framed_reader = FramedRead::new(reader, MozRdpStreamTransport::default());
    while let Some(msg) = channel.recv().await {
        writer.try_write(msg)
    }

    Ok(())
}

#[derive(Default)]
struct MozRdpStreamTransport {
    header: Option<MozRdpHeader>,
}

impl Decoder for MozRdpStreamTransport {
    type Item = RawMozRdpClientPacket;

    type Error = Box<dyn Error>;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let header = match &self.header {
            Some(header) => header,
            None => {
                let Some(position) = src.iter().position(|b| *b == b':') else {
                    if src.len() > 1000 {
                        // Input excessively long: assuming invalid packet
                        return Err(());
                    } else {
                        // Incomplete header
                        return Ok(None);
                    }
                };
                let header_input = &src[0..position];

                match MozRdpHeader::try_parse(header_input)? {
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
                let data = ByteString::try_from(data).map_err(|_| ())?;
                Ok(Some(RawMozRdpClientPacket::Json(data)))
            }
            MozRdpPacketKind::Bulk { actor, ty } => {
                let data = src.split_to(header.expected_data_length).freeze();
                Ok(Some(RawMozRdpClientPacket::Bulk(MozRdpClientPacket {
                    target_actor: actor,
                    packet_type: ty,
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
    Bulk { actor: String, ty: String },
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
            let actor = parts.next().ok_or(())?;
            let ty = parts.next().ok_or(())?;
            let length_str = parts.next().ok_or(())?;
            let length = length_str.parse().map_err(|_| ())?;

            return Ok(Some(Self {
                header_length: input.len() + 1,
                expected_data_length: length,
                header_kind: MozRdpPacketKind::Bulk {
                    actor: actor.to_string(),
                    ty: ty.to_string(),
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
    #[serde(rename = "to")]
    pub target_actor: String,
    #[serde(rename = "type")]
    pub packet_type: String,
    #[serde(flatten)]
    pub data: T,
}
