//! Handlers for the `Browser`, `Runtime` and `Page` domains: version info
//! and the minimal frame/navigation stubs the Elements panel needs to boot

use serde_json::json;

use super::{CdpError, Session, no_document, with_doc};
use crate::{CdpCommand, DocumentProvider, JsonValue, MessageWriter};

pub(super) fn dispatch(
    session: &mut Session,
    writer: &mut MessageWriter,
    docs: &mut dyn DocumentProvider,
    command: &CdpCommand,
) -> Result<JsonValue, CdpError> {
    let method = command.method.as_str();
    match method {
        "Browser.getVersion" => Ok(json!({
            "protocolVersion": "1.3",
            "product": "Blitz",
            "revision": "",
            "userAgent": "Blitz",
            "jsVersion": "",
        })),

        "Runtime.evaluate" | "Runtime.callFunctionOn" => {
            Ok(json!({ "result": { "type": "undefined" } }))
        }

        "Page.getResourceTree" | "Page.getFrameTree" => {
            let doc_id = session.doc_id(docs).ok_or_else(no_document)?;
            let url =
                with_doc(docs, doc_id, |doc| doc.url().to_string()).ok_or_else(no_document)?;
            let frame = json!({
                "id": format!("frame-{doc_id}"),
                "loaderId": format!("loader-{doc_id}"),
                "url": url,
                "domainAndRegistry": "",
                "securityOrigin": "",
                "mimeType": "text/html",
                "secureContextType": "Secure",
                "crossOriginIsolatedContextType": "NotIsolated",
                "gatedAPIFeatures": [],
            });
            match method {
                "Page.getResourceTree" => {
                    Ok(json!({ "frameTree": { "frame": frame, "resources": [] } }))
                }
                _ => Ok(json!({ "frameTree": { "frame": frame } })),
            }
        }
        "Page.getNavigationHistory" => Ok(json!({ "currentIndex": 0, "entries": [] })),

        // The screencast pane (shown by the chrome://inspect frontend)
        // is not supported: report it as not visible so its blank pane
        // shows an explanatory message rather than appearing broken.
        // Note that while the screencast is toggled on, the frontend
        // routes element picking and node highlighting to the screencast
        // view instead of the Overlay domain; toggling it off restores
        // protocol-based picking on the Blitz window itself.
        "Page.startScreencast" => {
            writer.event(
                "Page.screencastVisibilityChanged",
                json!({ "visible": false }),
            );
            Ok(json!({}))
        }

        _ => Err(CdpError::method_not_found(method)),
    }
}
