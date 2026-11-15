//! Loading SVG images (`<img src="x.svg">`, CSS images) together with the
//! external images their `<image>` and `<feImage>` elements reference.
//!
//! usvg resolves image hrefs synchronously while parsing, but Blitz fetches
//! asynchronously through its [`NetProvider`]. So the SVG's external hrefs are
//! collected from its XML first, resolved against the SVG's URL and fetched,
//! and the SVG is parsed once every fetch has finished, with a resolver that
//! reads the fetched bytes. usvg keeps the image bytes in the parsed tree, so
//! the loaded [`SvgImageData`] (cached per URL in the document's image cache)
//! is where the fetched images end up stored.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use blitz_traits::net::{Bytes, NetHandler};
use url::Url;

use super::{ImageHandler, Resource, ResourceHandler, stamped_request};
use crate::node::{SvgImageData, external_image_hrefs, parse_svg_xml, svg_text};
use crate::util::{FONT_DB, ImageType};

/// Fetch the external images of the SVG in `bytes` (which failed to decode as
/// a raster image with `image_err`) if the handler loads them, then parse it
/// and respond to `handler`.
pub(super) fn load(
    handler: Box<ResourceHandler<ImageHandler>>,
    svg_url: String,
    bytes: Bytes,
    image_err: String,
) {
    let kind = handler.data.kind;
    let parse_now = |handler: Box<ResourceHandler<ImageHandler>>,
                     svg_url: String,
                     bytes: Bytes,
                     image_err: String| {
        let result = parse(kind, &svg_url, &bytes, &HashMap::new(), &image_err);
        handler.respond(svg_url, result)
    };
    let Some((net_provider, signal)) = handler.data.svg_external_images.clone() else {
        return parse_now(handler, svg_url, bytes, image_err);
    };
    let urls = match external_image_urls(&svg_url, &bytes) {
        Ok(urls) => urls,
        Err(svg_err) => {
            let err = parse_error(&bytes, &image_err, &svg_err);
            return handler.respond(svg_url, Err(err));
        }
    };
    if urls.is_empty() || net_provider.is_noop() {
        return parse_now(handler, svg_url, bytes, image_err);
    }

    let doc_id = handler.doc_id;
    let pending = Arc::new(PendingSvgImage {
        kind,
        remaining: AtomicUsize::new(urls.len()),
        images: Mutex::new(HashMap::new()),
        handler: Mutex::new(Some(handler)),
        svg_url,
        bytes,
        image_err,
    });
    for url in urls {
        let request = stamped_request(url.clone(), signal.as_ref());
        let image_handler = ExternalImageHandler {
            pending: pending.clone(),
            url,
        };
        net_provider.fetch(doc_id, request, Box::new(image_handler));
    }
}

/// An SVG image waiting for the external images it references
struct PendingSvgImage {
    kind: ImageType,
    /// The number of external image fetches still in flight
    remaining: AtomicUsize,
    /// The fetched external images, by the URL they were requested with
    images: Mutex<HashMap<Url, Bytes>>,
    handler: Mutex<Option<Box<ResourceHandler<ImageHandler>>>>,
    svg_url: String,
    bytes: Bytes,
    image_err: String,
}

impl PendingSvgImage {
    fn finish_fetch(&self) {
        if self.remaining.fetch_sub(1, Ordering::AcqRel) != 1 {
            return;
        }
        let Some(handler) = self.handler.lock().unwrap().take() else {
            return;
        };
        let images = std::mem::take(&mut *self.images.lock().unwrap());
        let result = parse(
            self.kind,
            &self.svg_url,
            &self.bytes,
            &images,
            &self.image_err,
        );
        handler.respond(self.svg_url.clone(), result);
    }
}

/// Receives one external image of a [`PendingSvgImage`]. A fetch counts as
/// finished when the handler is dropped, so a provider that drops a request
/// without calling back doesn't leave the SVG pending forever.
struct ExternalImageHandler {
    pending: Arc<PendingSvgImage>,
    url: Url,
}

impl NetHandler for ExternalImageHandler {
    fn bytes(self: Box<Self>, _resolved_url: String, bytes: Bytes) {
        let url = self.url.clone();
        self.pending.images.lock().unwrap().insert(url, bytes);
    }
}

impl Drop for ExternalImageHandler {
    fn drop(&mut self) {
        self.pending.finish_fetch();
    }
}

fn resolve_href(base: Option<&Url>, href: &str) -> Option<Url> {
    match base {
        Some(base) => base.join(href).ok(),
        None => Url::parse(href).ok(),
    }
}

/// The distinct URLs of the external images an SVG references
fn external_image_urls(svg_url: &str, bytes: &[u8]) -> Result<Vec<Url>, usvg::Error> {
    let base = Url::parse(svg_url).ok();
    let text = svg_text(bytes)?;
    let doc = parse_svg_xml(&text)?;
    let urls: HashSet<Url> = external_image_hrefs(&doc)
        .filter_map(|href| resolve_href(base.as_ref(), href))
        .collect();
    Ok(urls.into_iter().collect())
}

fn parse(
    kind: ImageType,
    svg_url: &str,
    bytes: &[u8],
    images: &HashMap<Url, Bytes>,
    image_err: &str,
) -> Result<Resource, String> {
    let base = Url::parse(svg_url).ok();
    let options = usvg::Options {
        fontdb: Arc::clone(&*FONT_DB),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
            resolve_string: Box::new(|href, options| {
                let data = images.get(&resolve_href(base.as_ref(), href)?)?;
                external_image_kind(data, options)
            }),
        },
        ..Default::default()
    };
    SvgImageData::from_data(bytes, &options)
        .map(|svg| Resource::Svg(kind, svg))
        .map_err(|svg_err| parse_error(bytes, image_err, &svg_err))
}

/// An external image's bytes as an image usvg can render. Like usvg's own
/// resolver, an SVG referenced by an SVG image doesn't load its own external
/// images.
fn external_image_kind(data: &Bytes, options: &usvg::Options) -> Option<usvg::ImageKind> {
    use image::ImageFormat;
    let image_data = || Arc::new(data.to_vec());
    match image::guess_format(data) {
        Ok(ImageFormat::Png) => Some(usvg::ImageKind::PNG(image_data())),
        Ok(ImageFormat::Jpeg) => Some(usvg::ImageKind::JPEG(image_data())),
        Ok(ImageFormat::Gif) => Some(usvg::ImageKind::GIF(image_data())),
        Ok(ImageFormat::WebP) => Some(usvg::ImageKind::WEBP(image_data())),
        Ok(_) => None,
        Err(_) => usvg::Tree::from_data_nested(data, options)
            .ok()
            .map(usvg::ImageKind::SVG),
    }
}

fn parse_error(bytes: &[u8], image_err: &str, svg_err: &usvg::Error) -> String {
    format!(
        "Could not parse image ({} bytes): image-crate error: {image_err}; svg fallback error: {svg_err}",
        bytes.len()
    )
}
