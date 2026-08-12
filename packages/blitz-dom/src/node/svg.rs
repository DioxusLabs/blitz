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
        let view_box_size = root
            .attribute("viewBox")
            .and_then(|s| s.parse::<svgtypes::ViewBox>().ok())
            .filter(|vb| vb.w.is_finite() && vb.w > 0.0 && vb.h.is_finite() && vb.h > 0.0)
            .map(|vb| (vb.w as f32, vb.h as f32));

        Self {
            width: parse_length("width"),
            height: parse_length("height"),
            view_box_size,
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
        let xml_options = roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        };
        let doc = roxmltree::Document::parse_with_options(text, xml_options)
            .map_err(usvg::Error::ParsingFailed)?;
        let tree = usvg::Tree::from_xmltree(&doc, options)?;
        Ok(Self {
            tree: Arc::new(tree),
            intrinsic_dimensions: SvgIntrinsicDimensions::from_xmltree(&doc),
        })
    }

    /// The intrinsic width in CSS px, present only when the root `<svg>`
    /// declared an absolute (non-percentage) `width`.
    pub fn intrinsic_width(&self) -> Option<f32> {
        use svgtypes::LengthUnit;
        let declared = self
            .intrinsic_dimensions
            .width
            .is_some_and(|len| len.unit != LengthUnit::Percent);
        declared.then(|| self.tree.size().width())
    }

    /// The intrinsic height in CSS px, present only when the root `<svg>`
    /// declared an absolute (non-percentage) `height`.
    pub fn intrinsic_height(&self) -> Option<f32> {
        use svgtypes::LengthUnit;
        let declared = self
            .intrinsic_dimensions
            .height
            .is_some_and(|len| len.unit != LengthUnit::Percent);
        declared.then(|| self.tree.size().height())
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
            Some(len) if len.unit != LengthUnit::Percent => Some(self.tree.size().width()),
            Some(len) => container_width.map(|cw| cw * (len.number as f32) / 100.0),
            None => None,
        }
    }

    /// The root `height` attribute resolved against a containing block height.
    /// See [`Self::resolved_width`].
    pub fn resolved_height(&self, container_height: Option<f32>) -> Option<f32> {
        use svgtypes::LengthUnit;
        match self.intrinsic_dimensions.height {
            Some(len) if len.unit != LengthUnit::Percent => Some(self.tree.size().height()),
            Some(len) => container_height.map(|ch| ch * (len.number as f32) / 100.0),
            None => None,
        }
    }

    /// The intrinsic aspect ratio of the SVG: the ratio of its declared
    /// `width`/`height` when both are absolute lengths, otherwise the
    /// `viewBox` ratio, otherwise the ratio of the resolved
    /// [`usvg::Tree::size`] (which is always non-zero).
    pub fn aspect_ratio(&self) -> f32 {
        match (self.intrinsic_width(), self.intrinsic_height()) {
            (Some(w), Some(h)) => w / h,
            _ => self.viewbox_aspect_ratio().unwrap_or_else(|| {
                let size = self.tree.size();
                size.width() / size.height()
            }),
        }
    }

    /// The intrinsic dimensions of the SVG resolved per CSS replaced element
    /// sizing: a missing dimension is computed from the declared one and the
    /// intrinsic aspect ratio; if neither is declared, the resolved
    /// [`usvg::Tree::size`] is used as a fallback.
    pub fn intrinsic_size(&self) -> (f32, f32) {
        let aspect_ratio = self.aspect_ratio();
        match (self.intrinsic_width(), self.intrinsic_height()) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, w / aspect_ratio),
            (None, Some(h)) => (h * aspect_ratio, h),
            (None, None) => {
                // No intrinsic dimensions. If there is an intrinsic aspect ratio, apply
                // the CSS default sizing algorithm: contain within the default object
                // size of 300x150. Otherwise fall back to the resolved tree size.
                if self.viewbox_aspect_ratio().is_some() {
                    let scale = (300.0 / aspect_ratio).min(150.0);
                    (scale * aspect_ratio, scale)
                } else {
                    let size = self.tree.size();
                    (size.width(), size.height())
                }
            }
        }
    }
}
