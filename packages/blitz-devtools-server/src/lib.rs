//! (Firefox) devtools protocol server implementation

use bytes::{Bytes, BytesMut};
use bytestring::ByteString;
use serde::{Deserialize, Serialize};
use serde_json::json;
// use futures::SinkExt;
// use tokio_stream::StreamExt;
use std::error::Error;
use string::String as GenericString;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::{Decoder, Encoder, Framed};

async fn start_devtools_server() -> Result<(), Box<dyn Error>> {
    // Parse the arguments, bind the TCP socket we'll be listening to, spin up
    // our worker threads, and start shipping sockets to those worker threads.
    let addr = "127.0.0.1:6080";
    let server = TcpListener::bind(&addr).await?;
    println!("Listening on: {addr}");

    loop {
        let (stream, _) = server.accept().await?;
        tokio::spawn(async move {
            if let Err(e) = process(stream).await {
                println!("failed to process connection; error = {e}");
            }
        });
    }
}

async fn process(stream: TcpStream) -> Result<(), Box<dyn Error>> {
    let mut transport = Framed::new(stream, Http);

    while let Some(request) = transport.next().await {
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

struct MozRdpStreamTransport {
    header: Option<MozRdpHeader>,
}

impl Decoder for MozRdpStreamTransport {
    type Item = RawMozRdpPacket;

    type Error = ();

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
enum RawMozRdpClientPacket {
    Json(ByteString),
    Bulk(MozRdpClientPacket<Bytes>),
}

#[derive(Serialize, Deserialize)]
struct MozRdpClientPacket<T> {
    #[serde(rename = "to")]
    target_actor: String,
    #[serde(rename = "type")]
    packet_type: String,
    #[serde(flatten)]
    data: T,
}
