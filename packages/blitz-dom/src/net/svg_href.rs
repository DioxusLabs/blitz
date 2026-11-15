//! The external images that inline `<svg>` elements (including the root of
//! an SVG document) reference from `<image>` and `<feImage>`.
//!
//! usvg resolves image hrefs synchronously while parsing, but Blitz fetches
//! asynchronously through its [`NetProvider`](blitz_traits::net::NetProvider).
//! So parsing an `<svg>` fetches the hrefs it hasn't loaded yet, resolved
//! against the document's URL, into a per-URL store on the document, and
//! reconstructs the `<svg>` when they arrive, parsing it again with a resolver
//! that reads the store.
//!
//! SVG images (`<img src="x.svg">`, CSS images) don't load external
//! resources, as browsers render them in "secure animated mode".

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;

use blitz_traits::net::{Bytes, NetHandler};
use blitz_traits::node_id::NodeId;
use url::Url;

use super::{Resource, ResourceHandler};
use crate::BaseDocument;
use crate::node::{SvgImageData, external_image_hrefs, parse_svg_xml};
use crate::util::FONT_DB;

/// An external image an inline `<svg>` references, stored on the document by
/// URL so the `<svg>` can be re-parsed with it whenever it is reconstructed
pub(crate) enum SvgHrefImage {
    /// Being fetched, for these `<svg>` nodes
    Pending(Vec<NodeId>),
    Loaded(Bytes),
}

/// Parse the inline `<svg>` `node_id` from its serialized `source`, fetching
/// the external images it references that aren't loaded yet. `node_id` is
/// reconstructed once they arrive; until then it renders without them.
pub(crate) fn parse_inline_svg(
    doc: &mut BaseDocument,
    node_id: NodeId,
    source: &str,
) -> Result<SvgImageData, usvg::Error> {
    let xml = parse_svg_xml(source)?;
    let base = doc.url().clone();
    let mut images = HashMap::new();
    let mut to_fetch = Vec::new();
    for url in external_image_hrefs(&xml).filter_map(|href| resolve_href(Some(&base), href)) {
        match doc.svg_href_images.entry(url) {
            Entry::Occupied(mut entry) => match entry.get_mut() {
                SvgHrefImage::Loaded(bytes) => {
                    let bytes = bytes.clone();
                    images.insert(entry.key().clone(), bytes);
                }
                SvgHrefImage::Pending(nodes) => {
                    if !nodes.contains(&node_id) {
                        nodes.push(node_id);
                    }
                }
            },
            Entry::Vacant(entry) => {
                to_fetch.push(entry.key().clone());
                entry.insert(SvgHrefImage::Pending(vec![node_id]));
            }
        }
    }
    for url in to_fetch {
        doc.net_provider.fetch(
            doc.id(),
            doc.build_request(url.clone()),
            ResourceHandler::boxed(
                doc.tx.clone(),
                doc.id(),
                None,
                doc.shell_provider.clone(),
                SvgHrefImageHandler { url },
            ),
        );
    }
    SvgImageData::from_xmltree(&xml, &svg_options(Some(&base), &images))
}

/// Fetches an external image of an inline `<svg>`
pub(crate) struct SvgHrefImageHandler {
    /// The URL it was requested with, which keys [`SvgHrefImage`]s
    url: Url,
}

impl NetHandler for ResourceHandler<SvgHrefImageHandler> {
    fn bytes(self: Box<Self>, resolved_url: String, bytes: Bytes) {
        let url = self.data.url.clone();
        self.respond(resolved_url, Ok(Resource::SvgHrefImage(url, bytes)))
    }
}

/// usvg options that resolve an external image href (relative to `base`) to
/// its bytes in `images`, never to a local file.
fn svg_options<'a>(base: Option<&'a Url>, images: &'a HashMap<Url, Bytes>) -> usvg::Options<'a> {
    usvg::Options {
        fontdb: Arc::clone(&*FONT_DB),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
            resolve_string: Box::new(move |href, options| {
                let data = images.get(&resolve_href(base, href)?)?;
                external_image_kind(data, options)
            }),
        },
        ..Default::default()
    }
}

fn resolve_href(base: Option<&Url>, href: &str) -> Option<Url> {
    match base {
        Some(base) => base.join(href).ok(),
        None => Url::parse(href).ok(),
    }
}

/// An external image's bytes as an image usvg can render. Like usvg's own
/// resolver, an SVG image referenced by an SVG doesn't load its own external
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
