//! SVG image data and CSS intrinsic sizing for SVG.

use std::sync::Arc;

use usvg::roxmltree;

/// Dimensions declared on the root `<svg>` element, before any resolution.
///
/// Unlike [`usvg::Tree::size`], which always produces a concrete size, this
/// preserves what the SVG actually declared: absent attributes are `None` and
/// percentage lengths are kept unresolved.
#[derive(Debug, Clone, Copy, Default)]
pub struct SvgIntrinsicDimensions {
    /// The root `width` attribute, if declared. Percentages are unresolved.
    pub width: Option<svgtypes::Length>,
    /// The root `height` attribute, if declared. Percentages are unresolved.
    pub height: Option<svgtypes::Length>,
    /// The root `viewBox` width/height, if declared and valid.
    pub view_box_size: Option<(f32, f32)>,
    /// Whether the root declared a `viewBox` with a zero width or height,
    /// which disables rendering of the element per the SVG spec.
    pub degenerate_view_box: bool,
}

impl SvgIntrinsicDimensions {
    /// Extract the `width`/`height`/`viewBox` attributes declared on the root
    /// element of an already-parsed SVG document, with absent attributes as
    /// `None` and percentages unresolved. [`usvg::Tree`] does not preserve
    /// these (it always resolves to a concrete size), so they are read from
    /// the XML document here.
    pub fn from_xmltree(doc: &roxmltree::Document) -> Self {
        let root = doc.root_element();

        let parse_length = |name: &str| -> Option<svgtypes::Length> {
            root.attribute(name)?.parse::<svgtypes::Length>().ok()
        };
        // Parsed manually rather than via `svgtypes::ViewBox`, which rejects
        // zero sizes: a zero `viewBox` width/height is distinguished from an
        // invalid `viewBox` as it disables rendering of the element.
        let view_box_dims = root.attribute("viewBox").and_then(|s| {
            let mut numbers = svgtypes::NumberListParser::from(s);
            let _x = numbers.next()?.ok()?;
            let _y = numbers.next()?.ok()?;
            let w = numbers.next()?.ok()? as f32;
            let h = numbers.next()?.ok()? as f32;
            (numbers.next().is_none() && w.is_finite() && h.is_finite() && w >= 0.0 && h >= 0.0)
                .then_some((w, h))
        });
        let view_box_size = view_box_dims.filter(|&(w, h)| w > 0.0 && h > 0.0);
        let degenerate_view_box = view_box_dims.is_some_and(|(w, h)| w == 0.0 || h == 0.0);

        Self {
            width: parse_length("width"),
            height: parse_length("height"),
            view_box_size,
            degenerate_view_box,
        }
    }
}

/// A parsed SVG image.
///
/// usvg always resolves the root `<svg>` to a concrete [`usvg::Tree::size`],
/// falling back to the `viewBox` size when `width`/`height` are absent or given
/// as percentages. For CSS sizing purposes, however, such an SVG has *no*
/// intrinsic width/height (only an intrinsic aspect ratio). The accessors on
/// this type resolve the CSS intrinsic dimensions from the declared root
/// attributes, which are captured at parse time.
#[derive(Debug, Clone)]
pub struct SvgImageData {
    /// The parsed SVG tree.
    pub tree: Arc<usvg::Tree>,
    /// The dimensions declared on the root `<svg>` element.
    pub intrinsic_dimensions: SvgIntrinsicDimensions,
}

impl SvgImageData {
    /// Parse an SVG image from raw data, capturing both the rendered
    /// [`usvg::Tree`] and the declared root dimensions from a single XML
    /// parse.
    ///
    /// Like [`usvg::Tree::from_data`], gzip-compressed data (SVGZ) is
    /// decompressed first.
    pub fn from_data(data: &[u8], options: &usvg::Options) -> Result<Self, usvg::Error> {
        // Gzip magic bytes, matching the SVGZ detection in `usvg::Tree::from_data`.
        let decompressed;
        let data = if data.starts_with(&[0x1f, 0x8b]) {
            decompressed = usvg::decompress_svgz(data)?;
            decompressed.as_slice()
        } else {
            data
        };

        let text = std::str::from_utf8(data).map_err(|_| usvg::Error::NotAnUtf8Str)?;
        let xml_options = || roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        };
        let doc = roxmltree::Document::parse_with_options(text, xml_options())
            .map_err(usvg::Error::ParsingFailed)?;
        let intrinsic_dimensions = SvgIntrinsicDimensions::from_xmltree(&doc);

        // usvg refuses to parse an SVG whose root `width`/`height` resolve to
        // a zero or negative size, but such an SVG is still a valid image: the
        // degenerate attribute gives a zero intrinsic dimension, and CSS can
        // still size the element to something visible. Blank out the offending
        // attributes (their byte ranges are known from the parsed document) so
        // usvg resolves the viewport from the `viewBox`/defaults instead.
        let degenerate_attr_ranges: Vec<std::ops::Range<usize>> = doc
            .root_element()
            .attributes()
            .filter(|attr| {
                matches!(attr.name(), "width" | "height")
                    && attr
                        .value()
                        .parse::<svgtypes::Length>()
                        .is_ok_and(|len| len.number <= 0.0)
            })
            .map(|attr| attr.range())
            .collect();

        let tree = if degenerate_attr_ranges.is_empty() {
            usvg::Tree::from_xmltree(&doc, options)?
        } else {
            let mut patched = text.as_bytes().to_vec();
            for range in degenerate_attr_ranges {
                patched[range].fill(b' ');
            }
            let patched = std::str::from_utf8(&patched).map_err(|_| usvg::Error::NotAnUtf8Str)?;
            let patched_doc = roxmltree::Document::parse_with_options(patched, xml_options())
                .map_err(usvg::Error::ParsingFailed)?;
            usvg::Tree::from_xmltree(&patched_doc, options)?
        };

        Ok(Self {
            tree: Arc::new(tree),
            intrinsic_dimensions,
        })
    }

    /// The intrinsic width in CSS px, present only when the root `<svg>`
    /// declared an absolute (non-percentage) `width`. A zero or negative
    /// declared width is a zero intrinsic width (matching browsers), not an
    /// absent one.
    ///
    /// Viewport-relative lengths (`vw`/`vh`/`vmin`/`vmax`) fail to parse as an
    /// [`svgtypes::Length`] and so are already absent from
    /// [`SvgIntrinsicDimensions`]; like percentages, they contribute no
    /// intrinsic dimension.
    pub fn intrinsic_width(&self) -> Option<f32> {
        self.intrinsic_dimensions.width.and_then(resolve_absolute)
    }

    /// The intrinsic height in CSS px, present only when the root `<svg>`
    /// declared an absolute (non-percentage) `height`. See
    /// [`Self::intrinsic_width`].
    pub fn intrinsic_height(&self) -> Option<f32> {
        self.intrinsic_dimensions.height.and_then(resolve_absolute)
    }

    /// The aspect ratio of the root `<svg>`'s `viewBox`, if it declares one.
    pub fn viewbox_aspect_ratio(&self) -> Option<f32> {
        self.intrinsic_dimensions.view_box_size.map(|(w, h)| w / h)
    }

    /// The root `width` attribute resolved against a containing block width:
    /// percentages resolve against the containing block (`None` if it is
    /// indefinite) and an absent attribute is `None`.
    ///
    /// This is only appropriate for an inline `<svg>` element, where the
    /// attributes behave as presentation attributes. SVG used as an image
    /// (e.g. `<img src>` or a background) must use [`Self::intrinsic_width`],
    /// as its intrinsic dimensions are context-free per CSS.
    pub fn resolved_width(&self, container_width: Option<f32>) -> Option<f32> {
        use svgtypes::LengthUnit;
        match self.intrinsic_dimensions.width {
            Some(len) if len.unit != LengthUnit::Percent => resolve_absolute(len),
            Some(len) => container_width.map(|cw| cw * (len.number as f32) / 100.0),
            None => None,
        }
    }

    /// The root `height` attribute resolved against a containing block height.
    /// See [`Self::resolved_width`].
    pub fn resolved_height(&self, container_height: Option<f32>) -> Option<f32> {
        use svgtypes::LengthUnit;
        match self.intrinsic_dimensions.height {
            Some(len) if len.unit != LengthUnit::Percent => resolve_absolute(len),
            Some(len) => container_height.map(|ch| ch * (len.number as f32) / 100.0),
            None => None,
        }
    }

    /// The intrinsic aspect ratio of the SVG: the ratio of its declared
    /// `width`/`height` when both are (positive) absolute lengths, otherwise
    /// the `viewBox` ratio. An SVG with neither has no intrinsic aspect ratio:
    /// in particular the resolved [`usvg::Tree::size`] must not supply one, as
    /// it is a rendering fallback rather than an intrinsic dimension.
    pub fn aspect_ratio(&self) -> Option<f32> {
        match (self.intrinsic_width(), self.intrinsic_height()) {
            (Some(w), Some(h)) => (w > 0.0 && h > 0.0).then(|| w / h),
            _ => self.viewbox_aspect_ratio(),
        }
    }

    /// Whether rendering of the SVG content is disabled per the SVG spec: the
    /// root declared a zero/negative `width` or `height`, or a `viewBox` with
    /// a zero width or height. The element still generates a (potentially
    /// CSS-sized) box; only its content is not painted.
    pub fn rendering_disabled(&self) -> bool {
        self.intrinsic_dimensions.degenerate_view_box
            || self
                .intrinsic_dimensions
                .width
                .is_some_and(|len| len.number <= 0.0)
            || self
                .intrinsic_dimensions
                .height
                .is_some_and(|len| len.number <= 0.0)
    }

    /// The intrinsic dimensions of the SVG resolved per the CSS default
    /// sizing algorithm with no specified size
    /// (https://drafts.csswg.org/css-images/#default-sizing): a missing
    /// dimension is computed from the declared one and the intrinsic aspect
    /// ratio, falling back to the 300x150 default object size.
    pub fn intrinsic_size(&self) -> (f32, f32) {
        self.concrete_object_size((300.0, 150.0))
    }

    /// The concrete object size of the SVG per the CSS default sizing
    /// algorithm (https://drafts.csswg.org/css-images/#default-sizing) with no
    /// specified size: a missing dimension is computed from the declared one
    /// and the intrinsic aspect ratio, falling back to the given default
    /// object size.
    pub fn concrete_object_size(&self, default_object_size: (f32, f32)) -> (f32, f32) {
        let (default_width, default_height) = default_object_size;
        let ratio = self.aspect_ratio();
        match (self.intrinsic_width(), self.intrinsic_height()) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, ratio.map(|r| w / r).unwrap_or(default_height)),
            (None, Some(h)) => (ratio.map(|r| h * r).unwrap_or(default_width), h),
            (None, None) => match ratio {
                Some(ratio) => {
                    let scale = (default_width / ratio).min(default_height);
                    (scale * ratio, scale)
                }
                None => (default_width, default_height),
            },
        }
    }
}

/// Resolve an absolute (non-percentage) SVG length to CSS px, using the same
/// unit factors as usvg with default options (font-size 16px). Negative
/// lengths clamp to zero: browsers treat a negative root `width`/`height` as a
/// zero intrinsic dimension. Percentages resolve to `None` as they have no
/// context-free value.
fn resolve_absolute(len: svgtypes::Length) -> Option<f32> {
    use svgtypes::LengthUnit;
    let px_per_unit = match len.unit {
        LengthUnit::None | LengthUnit::Px => 1.0,
        LengthUnit::Em => 16.0,
        LengthUnit::Ex => 8.0,
        LengthUnit::In => 96.0,
        LengthUnit::Cm => 96.0 / 2.54,
        LengthUnit::Mm => 96.0 / 25.4,
        LengthUnit::Pt => 96.0 / 72.0,
        LengthUnit::Pc => 16.0,
        LengthUnit::Percent => return None,
    };
    Some((len.number as f32 * px_per_unit).max(0.0))
}
