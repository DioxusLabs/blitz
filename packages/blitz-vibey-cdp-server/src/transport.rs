//! Network transport: the TCP listener, HTTP discovery endpoints
//! (`/json/list`, `/json/version`) and the WebSocket upgrade / message loop.

use serde_json::json;
use std::collections::HashMap;
use std::error::Error;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel, sync_channel};
use std::time::Duration;
use tungstenite::Message;
use tungstenite::WebSocket;
use tungstenite::handshake::derive_accept_key;
use tungstenite::protocol::Role;

use crate::session::Session;
use crate::{CdpCommand, CdpEvent, CdpEventData, Connection, JsonValue, MessageWriter};

pub(crate) fn run_cdp_server(
    addr: String,
    sender: Arc<dyn Fn(CdpEvent) + Send + Sync>,
    local_addr: Arc<std::sync::Mutex<Option<std::net::SocketAddr>>>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let server = TcpListener::bind(&addr)?;
    let bound_addr = server.local_addr()?;
    *local_addr.lock().unwrap() = Some(bound_addr);
    tracing::info!("CDP: listening on: {addr}");

    let mut connection_id_counter: usize = 0;

    loop {
        let (stream, _) = server.accept()?;
        connection_id_counter += 1;
        let connection_id = connection_id_counter;
        let sender = Arc::clone(&sender);
        std::thread::spawn(move || {
            if let Err(err) = handle_connection(stream, connection_id, sender, bound_addr) {
                tracing::warn!("CDP: connection error: {err}");
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

    tracing::debug!("CDP: new connection (id: {connection_id}, path: {path})");

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
                    tracing::warn!("CDP: invalid JSON message: {text}");
                    continue;
                };
                let Some(method) = parsed.get("method").and_then(|m| m.as_str()) else {
                    tracing::warn!("CDP: message without method: {text}");
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
