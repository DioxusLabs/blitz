use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

use blitz_dom::{BaseDocument, local_name};
use blitz_traits::net::{Request, Url};

use crate::StdNetProvider;

pub fn find_favicon_url(document: &BaseDocument) -> Option<String> {
    document.tree().iter().find_map(|(_, node)| {
        let data = &node.data;
        if !data.is_element_with_tag_name(&local_name!("link")) {
            return None;
        }
        let rel = data.attr(local_name!("rel"))?;
        if !rel
            .split_ascii_whitespace()
            .any(|v| v.eq_ignore_ascii_case("icon"))
        {
            return None;
        }
        data.attr(local_name!("href")).map(|s| s.to_string())
    })
}

// Resolved favicon URLs that have been confirmed to fetch as image bytes.
// Shared across all tabs so navigating around the same site doesn't refetch
// favicon.ico repeatedly. Only positive results are cached — a transient
// probe failure (network blip, slow TLS handshake) must not poison the
// entry for the rest of the process lifetime.
static FAVICON_CACHE: LazyLock<RwLock<HashMap<Url, Url>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

pub async fn resolve_favicon_url(
    base_url: &str,
    favicon_href: Option<&str>,
    net_provider: &Arc<StdNetProvider>,
) -> Option<Url> {
    let base = Url::parse(base_url).ok()?;
    let candidate = favicon_href
        .and_then(|href| base.join(href).ok())
        .or_else(|| base.join("/favicon.ico").ok())?;

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
    // success from the response body. Without this, sites missing
    // /favicon.ico would render a broken <img> in the tab strip.
    let (_, bytes) = net_provider
        .fetch_async(Request::get(url.clone()))
        .await
        .ok()?;
    if bytes.is_empty() || !looks_like_image(&bytes) {
        return None;
    }
    Some(url.clone())
}

fn looks_like_image(bytes: &[u8]) -> bool {
    looks_like_raster_image(bytes) || looks_like_svg(bytes)
}

fn looks_like_raster_image(bytes: &[u8]) -> bool {
    // Magic-byte check for the formats favicons commonly take. We avoid
    // pulling decoder feature flags (e.g. `image/ico`) just to answer a
    // yes/no sniff question.
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    const GIF87: &[u8] = b"GIF87a";
    const GIF89: &[u8] = b"GIF89a";
    const BMP: &[u8] = b"BM";
    const ICO: &[u8] = b"\x00\x00\x01\x00";
    const CUR: &[u8] = b"\x00\x00\x02\x00";
    const JPEG_SOI: &[u8] = b"\xff\xd8\xff";

    if bytes.starts_with(PNG)
        || bytes.starts_with(GIF87)
        || bytes.starts_with(GIF89)
        || bytes.starts_with(BMP)
        || bytes.starts_with(ICO)
        || bytes.starts_with(CUR)
        || bytes.starts_with(JPEG_SOI)
    {
        return true;
    }
    bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP"
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    // Trim a UTF-8 BOM and leading whitespace, then scan a generous window
    // for `<svg` (case-insensitive). Reject HTML payloads up front so a 404
    // page served at /favicon.ico doesn't get sniffed as an icon.
    const BOM: &[u8] = b"\xef\xbb\xbf";
    let mut head = bytes;
    if head.starts_with(BOM) {
        head = &head[BOM.len()..];
    }
    while let Some((&b, rest)) = head.split_first() {
        if b.is_ascii_whitespace() {
            head = rest;
        } else {
            break;
        }
    }
    let window = &head[..head.len().min(2048)];
    // Note: a raw-text `<html>` string inside an XML comment before `<svg>`
    // would cause a false negative here. Acceptable for favicon sniffing.
    if find_ignore_ascii_case(window, b"<!doctype html").is_some()
        || find_ignore_ascii_case(window, b"<html").is_some()
    {
        return false;
    }
    find_ignore_ascii_case(window, b"<svg").is_some()
}

fn find_ignore_ascii_case(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len())
        .position(|w| w.iter().zip(needle).all(|(a, b)| a.eq_ignore_ascii_case(b)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_raster_formats() {
        assert!(looks_like_image(b"\x89PNG\r\n\x1a\nrest"));
        assert!(looks_like_image(b"GIF89aXXX"));
        assert!(looks_like_image(b"\xff\xd8\xff\xe0jfif"));
        assert!(looks_like_image(b"\x00\x00\x01\x00ico"));
        assert!(looks_like_image(b"RIFF\x00\x00\x00\x00WEBPvp80"));
    }

    #[test]
    fn sniffs_svg_with_prolog_and_bom() {
        assert!(looks_like_image(b"<svg xmlns=\"...\"></svg>"));
        assert!(looks_like_image(
            b"<?xml version=\"1.0\"?>\n<!-- c -->\n<svg></svg>"
        ));
        assert!(looks_like_image(b"\xef\xbb\xbf  <svg/>"));
    }

    #[test]
    fn rejects_html_payload() {
        assert!(!looks_like_image(
            b"<!DOCTYPE html><html><body>Not Found</body></html>"
        ));
        assert!(!looks_like_image(b"<html><svg></svg></html>"));
    }

    #[test]
    fn rejects_empty_or_garbage() {
        assert!(!looks_like_image(b""));
        assert!(!looks_like_image(b"hello world"));
    }
}
