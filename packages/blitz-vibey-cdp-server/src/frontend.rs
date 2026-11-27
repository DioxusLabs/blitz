//! Serving a DevTools frontend: the copy embedded by the `embedded-frontend`
//! feature (see [`blitz_chrome_devtools_frontend`]), or a local build of it.

use std::borrow::Cow;
use std::path::Path;

/// Environment variable naming a directory to serve the frontend from
/// instead of the embedded copy (e.g. the `dist/` of a local
/// blitz-chrome-devtools-frontend build)
const FRONTEND_DIR_VAR: &str = "BLITZ_DEVTOOLS_FRONTEND_DIR";

/// The path of the frontend's page
const ENTRY_PATH: &str = "devtools_app.html";

/// The URL of a DevTools frontend inspecting the target `target_id` (see
/// [`target_id`](crate::target_id)) of the server listening on `addr`.
///
/// When the `embedded-frontend` feature is enabled (or
/// `BLITZ_DEVTOOLS_FRONTEND_DIR` is set), this is the frontend served by the
/// server itself, which browsers such as Chrome and Firefox can open.
/// Otherwise it's the frontend bundled with Chrome (a `devtools://` URL, as
/// advertised at `/json/list`), which only Chrome can open: by launching Chrome
/// with it (where that's possible, e.g. on macOS) or pasting it into the
/// address bar.
pub fn frontend_url(addr: std::net::SocketAddr, target_id: &str) -> String {
    let serves_frontend =
        cfg!(feature = "embedded-frontend") || std::env::var_os(FRONTEND_DIR_VAR).is_some();
    let base = if serves_frontend {
        format!("http://{addr}")
    } else {
        "devtools://devtools/bundled".to_string()
    };
    format!("{base}/{ENTRY_PATH}?ws={addr}/devtools/page/{target_id}")
}

/// The `Content-Type` and contents of the frontend file at `path` (relative
/// to the frontend's root), if the server serves a frontend and it has one
pub(crate) fn file(path: &str) -> Option<(&'static str, Cow<'static, [u8]>)> {
    if path
        .split('/')
        .any(|segment| segment.is_empty() || segment == "..")
    {
        return None;
    }
    let body = match std::env::var_os(FRONTEND_DIR_VAR) {
        Some(dir) => Cow::Owned(std::fs::read(Path::new(&dir).join(path)).ok()?),
        None => Cow::Borrowed(embedded_file(path)?),
    };
    Some((content_type(path), body))
}

#[cfg(feature = "embedded-frontend")]
fn embedded_file(path: &str) -> Option<&'static [u8]> {
    use std::collections::HashMap;
    use std::sync::OnceLock;

    // Unpacked on first use (about 11 MB) and kept for the process lifetime
    static FILES: OnceLock<HashMap<String, Vec<u8>>> = OnceLock::new();
    let files = FILES.get_or_init(|| {
        blitz_chrome_devtools_frontend::files().unwrap_or_else(|err| {
            tracing::warn!("CDP: failed to unpack the DevTools frontend: {err}");
            HashMap::new()
        })
    });
    files.get(path).map(Vec::as_slice)
}

#[cfg(not(feature = "embedded-frontend"))]
fn embedded_file(_path: &str) -> Option<&'static [u8]> {
    None
}

/// Browsers refuse to run module scripts not served with a JavaScript MIME type
fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, ext)| ext) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("avif") => "image/avif",
        Some("ico") => "image/x-icon",
        Some("wasm") => "application/wasm",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}
