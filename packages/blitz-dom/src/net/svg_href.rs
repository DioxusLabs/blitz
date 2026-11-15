//! The external images that inline `<svg>` elements (including the root of
//! an SVG document) reference from `<image>` and `<feImage>`.
//!
//! usvg resolves image hrefs synchronously while parsing, but Blitz fetches
//! asynchronously. So constructing an `<svg>` fetches the images that aren't
//! in the document's image cache like any other image, and the `<svg>` is
//! reconstructed when they arrive, parsing it again with a resolver that
//! reads the cache.
//!
//! SVG images (`<img src="x.svg">`, CSS images) don't load external
//! resources, as browsers render them in "secure animated mode".

use std::collections::HashMap;
use std::sync::Arc;

use blitz_traits::node_id::NodeId;
use markup5ever::{local_name, ns};

use crate::BaseDocument;
use crate::node::{ImageData, RasterImageData, SvgImageData};
use crate::util::{FONT_DB, ImageType};

/// Parse the inline `<svg>` `node_id` from its serialized `source`, fetching
/// the external images it references that aren't loaded yet. `node_id` is
/// reconstructed once they arrive; until then it renders without them.
pub(crate) fn parse_inline_svg(
    doc: &mut BaseDocument,
    node_id: NodeId,
    source: &str,
) -> Result<SvgImageData, usvg::Error> {
    let mut images = HashMap::new();
    for url in external_image_urls(doc, node_id) {
        if images.contains_key(url.as_str()) {
            continue;
        }
        match doc.image_cache.get(url.as_str()) {
            Some(image) => {
                images.insert(url.to_string(), image.clone());
            }
            None => doc.fetch_image(url, node_id, ImageType::SvgHref),
        }
    }

    let base_url = doc.url.clone();
    let options = usvg::Options {
        fontdb: Arc::clone(&*FONT_DB),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
            // Only ever the fetched images, never a local file
            resolve_string: Box::new(move |href, _| {
                image_kind(images.get(base_url.resolve_relative(href)?.as_str())?)
            }),
        },
        ..Default::default()
    };
    SvgImageData::from_data(source.as_bytes(), &options)
}

/// The resolved URLs of the `<image>` and `<feImage>` hrefs in the subtree of
/// `node_id` that point outside the document: neither `data:` URLs nor
/// `#fragment`s. These are the hrefs usvg asks `resolve_string` for.
fn external_image_urls(doc: &BaseDocument, node_id: NodeId) -> Vec<url::Url> {
    let mut urls = Vec::new();
    let mut stack = vec![node_id];
    while let Some(id) = stack.pop() {
        let node = &doc.nodes[id];
        stack.extend(node.children.iter().copied());
        let Some(element) = node.element_data() else {
            continue;
        };
        if element.name.ns != ns!(svg) || !matches!(&*element.name.local, "image" | "feImage") {
            continue;
        }
        // SVG 2: an unprefixed `href` takes precedence over `xlink:href`.
        let href_in = |namespace| {
            element
                .attrs()
                .iter()
                .find(|attr| attr.name.local == local_name!("href") && attr.name.ns == namespace)
                .map(|attr| attr.value.as_str())
        };
        let Some(href) = href_in(ns!()).or_else(|| href_in(ns!(xlink))) else {
            continue;
        };
        if is_external(href)
            && let Some(url) = doc.resolve_url(href)
        {
            urls.push(url);
        }
    }
    urls
}

fn is_external(href: &str) -> bool {
    let href = href.trim_start();
    !href.starts_with('#')
        && !href
            .get(..5)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("data:"))
}

/// A loaded image as an image usvg can render
fn image_kind(image: &ImageData) -> Option<usvg::ImageKind> {
    match image {
        // TODO: hand usvg the decoded pixels instead of re-encoding them once
        // usvg supports that: https://github.com/linebender/resvg/issues/1096
        ImageData::Raster(raster) => {
            encode_png(raster).map(|png| usvg::ImageKind::PNG(Arc::new(png)))
        }
        ImageData::Svg(svg) => Some(usvg::ImageKind::SVG((*svg.tree).clone())),
        ImageData::None => None,
    }
}

fn encode_png(image: &RasterImageData) -> Option<Vec<u8>> {
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fastest);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(image.data.data()).ok()?;
    writer.finish().ok()?;
    Some(png)
}
