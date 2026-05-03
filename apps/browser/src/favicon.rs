use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, LazyLock, RwLock};

use blitz_traits::net::{Request, Url};

use crate::StdNetProvider;

// Resolved favicon URLs that have been confirmed to fetch as image bytes.
// Shared across all tabs so navigating around the same site doesn't refetch
// favicon.ico repeatedly. Only positive results are cached — a transient
// probe failure (network blip, slow TLS handshake) must not poison the
// entry for the rest of the process lifetime.
static FAVICON_CACHE: LazyLock<RwLock<HashMap<Url, Url>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

// Synchronously resolve which URL we'd probe, without doing any I/O. This is
// what the document loader hands to the tab so the load can return immediately;
// the actual probe runs in the background via `probe_favicon_cached`.
pub fn favicon_candidate(base_url: &str, favicon_href: Option<&str>) -> Option<Url> {
    let base = Url::parse(base_url).ok()?;
    favicon_href
        .and_then(|href| base.join(href).ok())
        .or_else(|| base.join("/favicon.ico").ok())
}

pub async fn probe_favicon_cached(
    candidate: Url,
    net_provider: &Arc<StdNetProvider>,
) -> Option<Url> {
    if let Ok(cache) = FAVICON_CACHE.read() {
        if let Some(cached) = cache.get(&candidate) {
            return Some(cached.clone());
        }
    }

    let resolved = probe_favicon(&candidate, net_provider).await?;
    if let Ok(mut cache) = FAVICON_CACHE.write() {
        cache.insert(candidate, resolved.clone());
    }
    Some(resolved)
}

async fn probe_favicon(url: &Url, net_provider: &Arc<StdNetProvider>) -> Option<Url> {
    // The net layer doesn't surface HTTP status or Content-Type, so we infer
    // success by attempting to actually decode the response body. Without
    // this, sites missing /favicon.ico would render a broken <img> in the
    // tab strip.
    let (_, bytes) = net_provider
        .fetch_async(Request::get(url.clone()))
        .await
        .ok()?;
    if bytes.is_empty() || !is_decodable_image(&bytes) {
        return None;
    }
    Some(url.clone())
}

// Mirrors blitz-dom's ImageHandler::parse: try the `image` crate first, then
// fall back to usvg for SVG favicons. If neither succeeds the renderer
// wouldn't be able to display it either, so we treat the probe as failed.
fn is_decodable_image(bytes: &[u8]) -> bool {
    let raster_ok = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .is_some_and(|r| r.decode().is_ok());
    raster_ok || usvg::Tree::from_data(bytes, &usvg::Options::default()).is_ok()
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn make_png_1x1() -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
        let mut bytes = Vec::new();
        img.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("encoding 1x1 png cannot fail");
        bytes
    }

    #[test]
    fn accepts_real_png() {
        assert!(is_decodable_image(&make_png_1x1()));
    }

    #[test]
    fn accepts_real_svg() {
        assert!(is_decodable_image(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>"#
        ));
    }

    #[test]
    fn rejects_html_payload() {
        assert!(!is_decodable_image(
            b"<!DOCTYPE html><html><body>Not Found</body></html>"
        ));
    }

    #[test]
    fn rejects_truncated_png_with_valid_header() {
        // Valid PNG magic bytes but no IHDR — sniffing would accept this, decoding rejects it.
        assert!(!is_decodable_image(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn rejects_empty_or_garbage() {
        assert!(!is_decodable_image(b""));
        assert!(!is_decodable_image(b"hello world"));
    }
}
