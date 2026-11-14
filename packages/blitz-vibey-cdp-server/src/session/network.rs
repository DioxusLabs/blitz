//! Handlers for the `Network` domain: request lifecycle events for the
//! requests of the inspected document (and its sub-documents), and response
//! bodies kept (up to a per-session cap) for `Network.getResponseBody`

use std::collections::VecDeque;

use blitz_traits::net::Bytes;
use serde_json::json;

use super::{CdpError, Session};
use crate::network::{NetworkRecord, base64_encode, is_text_mime_type};
use crate::{CdpCommand, JsonValue, MessageWriter};

/// Body buffer sizes used when `Network.enable` doesn't specify them
const DEFAULT_MAX_TOTAL_BUFFER_SIZE: usize = 50 * 1024 * 1024;
const DEFAULT_MAX_RESOURCE_BUFFER_SIZE: usize = 10 * 1024 * 1024;

/// The state of a session with the `Network` domain enabled
pub(crate) struct NetworkState {
    /// Response bodies, oldest first
    bodies: VecDeque<StoredBody>,
    total_size: usize,
    max_total_size: usize,
    max_resource_size: usize,
}

struct StoredBody {
    request_id: u64,
    mime_type: &'static str,
    bytes: Bytes,
}

impl NetworkState {
    fn store_body(&mut self, request_id: u64, mime_type: &'static str, bytes: &Bytes) {
        if bytes.len() > self.max_resource_size || bytes.len() > self.max_total_size {
            return;
        }
        while self.total_size + bytes.len() > self.max_total_size {
            let Some(evicted) = self.bodies.pop_front() else {
                break;
            };
            self.total_size -= evicted.bytes.len();
        }
        self.total_size += bytes.len();
        self.bodies.push_back(StoredBody {
            request_id,
            mime_type,
            bytes: bytes.clone(),
        });
    }
}

/// The CDP id of a request
fn cdp_request_id(request_id: u64) -> String {
    format!("blitz-{request_id}")
}

pub(super) fn dispatch(session: &mut Session, command: &CdpCommand) -> Result<JsonValue, CdpError> {
    let method = command.method.as_str();
    match method {
        "Network.enable" => {
            let size_param = |key: &str, default: usize| {
                command
                    .params
                    .get(key)
                    .and_then(|size| size.as_u64())
                    .map_or(default, |size| size as usize)
            };
            session.network = Some(NetworkState {
                bodies: VecDeque::new(),
                total_size: 0,
                max_total_size: size_param("maxTotalBufferSize", DEFAULT_MAX_TOTAL_BUFFER_SIZE),
                max_resource_size: size_param(
                    "maxResourceBufferSize",
                    DEFAULT_MAX_RESOURCE_BUFFER_SIZE,
                ),
            });
            Ok(json!({}))
        }
        "Network.disable" => {
            session.network = None;
            Ok(json!({}))
        }
        "Network.getResponseBody" => {
            let request_id = command
                .params
                .get("requestId")
                .and_then(|id| id.as_str())
                .ok_or_else(|| CdpError::invalid_params("Missing requestId"))?;
            let body = session
                .network
                .as_ref()
                .and_then(|network| {
                    network
                        .bodies
                        .iter()
                        .find(|body| cdp_request_id(body.request_id) == request_id)
                })
                .ok_or_else(|| {
                    CdpError::server_error("No data found for resource with given identifier")
                })?;
            if is_text_mime_type(body.mime_type)
                && let Ok(text) = std::str::from_utf8(&body.bytes)
            {
                return Ok(json!({ "body": text, "base64Encoded": false }));
            }
            Ok(json!({ "body": base64_encode(&body.bytes), "base64Encoded": true }))
        }
        // Request interception, throttling, cache control, cookies, etc. are
        // not supported
        _ => Ok(json!({})),
    }
}

impl Session {
    /// Whether this session has the `Network` domain enabled
    pub(crate) fn network_enabled(&self) -> bool {
        self.network.is_some()
    }

    /// Emit the `Network` events for a record of a request issued by the
    /// document with the given id and URL
    pub(crate) fn emit_network_record(
        &mut self,
        writer: &mut MessageWriter,
        doc_id: usize,
        doc_url: &str,
        record: &NetworkRecord,
    ) {
        let Some(network) = &mut self.network else {
            return;
        };
        let frame_id = format!("frame-{doc_id}");
        let loader_id = format!("loader-{doc_id}");
        match record {
            NetworkRecord::RequestWillBeSent(request) => {
                let headers: serde_json::Map<String, JsonValue> = request
                    .headers
                    .iter()
                    .map(|(name, value)| (name.clone(), json!(value)))
                    .collect();
                writer.event(
                    "Network.requestWillBeSent",
                    json!({
                        "requestId": cdp_request_id(request.request_id),
                        "loaderId": loader_id,
                        "documentURL": doc_url,
                        "request": {
                            "url": request.url,
                            "method": request.method,
                            "headers": headers,
                            "initialPriority": "High",
                            "referrerPolicy": "strict-origin-when-cross-origin",
                        },
                        "timestamp": request.timestamp,
                        "wallTime": request.wall_time,
                        "initiator": { "type": "other" },
                        "redirectHasExtraInfo": false,
                        "type": request.resource_type.as_str(),
                        "frameId": frame_id,
                        "hasUserGesture": false,
                    }),
                );
            }
            NetworkRecord::ResponseReceived(response) => {
                let request_id = cdp_request_id(response.request_id);
                let size = response.body.len();
                writer.event(
                    "Network.responseReceived",
                    json!({
                        "requestId": request_id,
                        "loaderId": loader_id,
                        "timestamp": response.timestamp,
                        "type": response.resource_type.as_str(),
                        "response": {
                            "url": response.url,
                            "status": 200,
                            "statusText": "OK",
                            "headers": {},
                            "mimeType": response.mime_type,
                            "connectionReused": false,
                            "connectionId": 0,
                            "encodedDataLength": size,
                            "securityState": "unknown",
                        },
                        "hasExtraInfo": false,
                        "frameId": frame_id,
                    }),
                );
                writer.event(
                    "Network.dataReceived",
                    json!({
                        "requestId": request_id,
                        "timestamp": response.timestamp,
                        "dataLength": size,
                        "encodedDataLength": size,
                    }),
                );
                writer.event(
                    "Network.loadingFinished",
                    json!({
                        "requestId": request_id,
                        "timestamp": response.timestamp,
                        "encodedDataLength": size,
                    }),
                );
                network.store_body(response.request_id, response.mime_type, &response.body);
            }
            NetworkRecord::LoadingFailed(failure) => {
                writer.event(
                    "Network.loadingFailed",
                    json!({
                        "requestId": cdp_request_id(failure.request_id),
                        "timestamp": failure.timestamp,
                        "type": failure.resource_type.as_str(),
                        "errorText": failure.error_text,
                        "canceled": failure.canceled,
                    }),
                );
            }
        }
    }
}
