//! SVG image data and CSS intrinsic sizing for SVG.

#[cfg(feature = "svg-usvg")]
use std::sync::Arc;

#[cfg(feature = "svg-usvg")]
use usvg::roxmltree;

/// A non-degenerate `viewBox` rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvgViewBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A 2D affine transform in SVG matrix order (`a b c d e f`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvgTransform {
    pub sx: f32,
    pub ky: f32,
    pub kx: f32,
    pub sy: f32,
    pub tx: f32,
    pub ty: f32,
}

impl SvgTransform {
    pub const IDENTITY: Self = Self {
        sx: 1.0,
        ky: 0.0,
        kx: 0.0,
        sy: 1.0,
        tx: 0.0,
        ty: 0.0,
    };
}

#[cfg(feature = "svg-usvg")]
impl From<SvgTransform> for usvg::Transform {
    fn from(t: SvgTransform) -> Self {
        usvg::Transform::from_row(t.sx, t.ky, t.kx, t.sy, t.tx, t.ty)
    }
}

/// Dimensions declared on the root `<svg>` element, before any resolution.
///
/// Unlike usvg's resolved tree size, which is always concrete, this preserves
/// what the SVG actually declared: absent attributes are `None` and percentage
/// lengths are kept unresolved.
#[derive(Debug, Clone, Copy, Default)]
pub struct SvgIntrinsicDimensions {
    /// The root `width` attribute, if declared. Percentages are unresolved.
    pub width: Option<svgtypes::Length>,
    /// The root `height` attribute, if declared. Percentages are unresolved.
    pub height: Option<svgtypes::Length>,
    /// The root `viewBox`, if declared and valid.
    pub view_box: Option<SvgViewBox>,
    /// The root `preserveAspectRatio` (default `xMidYMid meet`).
    pub preserve_aspect_ratio: svgtypes::AspectRatio,
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
    #[cfg(feature = "svg-usvg")]
    pub fn from_xmltree(doc: &roxmltree::Document) -> Self {
        let root = doc.root_element();
        Self::from_attrs(|name| root.attribute(name))
    }

    /// Extract the declared dimensions from an attribute lookup, used both for
    /// parsed SVG documents and for live inline `<svg>` DOM elements.
    pub fn from_attrs<'a>(attr: impl Fn(&str) -> Option<&'a str>) -> Self {
        let parse_length = |name: &str| -> Option<svgtypes::Length> {
            attr(name)?.parse::<svgtypes::Length>().ok()
        };
        // Parsed manually rather than via `svgtypes::ViewBox`, which rejects
        // zero sizes: a zero `viewBox` width/height is distinguished from an
        // invalid `viewBox` as it disables rendering of the element.
        let view_box_dims = attr("viewBox").and_then(|s| {
            let mut numbers = svgtypes::NumberListParser::from(s);
            let x = numbers.next()?.ok()? as f32;
            let y = numbers.next()?.ok()? as f32;
            let w = numbers.next()?.ok()? as f32;
            let h = numbers.next()?.ok()? as f32;
            (numbers.next().is_none() && w.is_finite() && h.is_finite() && w >= 0.0 && h >= 0.0)
                .then_some((x, y, w, h))
        });
        let view_box = view_box_dims.and_then(|(x, y, width, height)| {
            (width > 0.0 && height > 0.0).then_some(SvgViewBox {
                x,
                y,
                width,
                height,
            })
        });
        let degenerate_view_box = view_box_dims.is_some_and(|(_, _, w, h)| w == 0.0 || h == 0.0);
        let preserve_aspect_ratio = attr("preserveAspectRatio")
            .and_then(|s| s.parse().ok())
            .unwrap_or_default();

        Self {
            width: parse_length("width"),
            height: parse_length("height"),
            view_box,
            preserve_aspect_ratio,
            degenerate_view_box,
        }
    }

    /// The root `width` attribute in CSS px when it is an absolute length
    /// (`px`, `in`, `cm`, `mm`, `pt`, `pc` or unitless). Percentages and
    /// font-relative units are `None`: they are not intrinsic.
    pub fn absolute_width(&self) -> Option<f32> {
        self.width.and_then(absolute_length_px)
    }

    /// The root `height` attribute in CSS px when it is an absolute length.
    /// See [`Self::absolute_width`].
    pub fn absolute_height(&self) -> Option<f32> {
        self.height.and_then(absolute_length_px)
    }

    /// The intrinsic aspect ratio: the ratio of the absolute `width`/`height`
    /// when both are declared, otherwise the `viewBox` ratio, otherwise `None`.
    /// <https://svgwg.org/svg2-draft/coords.html#SizingSVGInCSS>
    pub fn intrinsic_aspect_ratio(&self) -> Option<f32> {
        match (self.absolute_width(), self.absolute_height()) {
            (Some(w), Some(h)) if h > 0.0 => Some(w / h),
            _ => self.view_box.map(|vb| vb.width / vb.height),
        }
    }

    /// The transform from `viewBox` user space onto a viewport of the given
    /// size, applying `preserveAspectRatio`. Identity without a `viewBox`.
    pub fn viewport_transform(&self, width: f32, height: f32) -> SvgTransform {
        match self.view_box {
            Some(view_box) => {
                view_box_transform(view_box, self.preserve_aspect_ratio, width, height)
            }
            None => SvgTransform::IDENTITY,
        }
    }
}

/// Convert an absolute SVG length to CSS px (`None` for relative units).
fn absolute_length_px(len: svgtypes::Length) -> Option<f32> {
    use svgtypes::LengthUnit;
    let n = len.number as f32;
    Some(match len.unit {
        LengthUnit::None | LengthUnit::Px => n,
        LengthUnit::In => n * 96.0,
        LengthUnit::Cm => n * 96.0 / 2.54,
        LengthUnit::Mm => n * 96.0 / 25.4,
        LengthUnit::Pt => n * 96.0 / 72.0,
        LengthUnit::Pc => n * 16.0,
        LengthUnit::Em | LengthUnit::Ex | LengthUnit::Percent => return None,
    })
}

/// A parsed SVG image.
///
/// usvg always resolves the root `<svg>` to a concrete [`usvg::Tree::size`],
/// falling back to the `viewBox` size when `width`/`height` are absent or given
/// as percentages. For CSS sizing purposes, however, such an SVG has *no*
/// intrinsic width/height (only an intrinsic aspect ratio). The accessors on
/// this type resolve the CSS intrinsic dimensions from the declared root
/// attributes, which are captured at parse time.
#[cfg(feature = "svg-usvg")]
#[derive(Debug, Clone)]
pub struct SvgImageData {
    /// The parsed SVG tree.
    pub tree: Arc<usvg::Tree>,
    /// The dimensions declared on the root `<svg>` element.
    pub intrinsic_dimensions: SvgIntrinsicDimensions,
}

#[cfg(feature = "svg-usvg")]
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
        self.intrinsic_dimensions
            .view_box
            .map(|view_box| view_box.width / view_box.height)
    }

    /// The transform from the rendered tree (in [`usvg::Tree::size`] units) to a
    /// viewport of the given size, applying the root `viewBox` and
    /// `preserveAspectRatio`. Without a `viewBox` the tree is stretched to fill it.
    pub fn viewport_transform(&self, width: f32, height: f32) -> usvg::Transform {
        let size = self.tree.size();
        let stretch = usvg::Transform::from_scale(width / size.width(), height / size.height());
        let Some(view_box) = self.intrinsic_dimensions.view_box else {
            return stretch;
        };
        let aspect = self.intrinsic_dimensions.preserve_aspect_ratio;
        // usvg has already mapped the `viewBox` onto `Tree::size`, so undo that first.
        let view_box_to_tree: usvg::Transform =
            view_box_transform(view_box, aspect, size.width(), size.height()).into();
        match view_box_to_tree.invert() {
            Some(tree_to_view_box) => {
                usvg::Transform::from(view_box_transform(view_box, aspect, width, height))
                    .pre_concat(tree_to_view_box)
            }
            None => stretch,
        }
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

/// Map a `viewBox` onto a viewport of the given size per `preserveAspectRatio`.
fn view_box_transform(
    view_box: SvgViewBox,
    aspect: svgtypes::AspectRatio,
    width: f32,
    height: f32,
) -> SvgTransform {
    use svgtypes::Align;

    let sx = width / view_box.width;
    let sy = height / view_box.height;
    let (sx, sy) = if aspect.align == Align::None {
        (sx, sy)
    } else if aspect.slice {
        (sx.max(sy), sx.max(sy))
    } else {
        (sx.min(sy), sx.min(sy))
    };
    let (align_x, align_y) = match aspect.align {
        Align::None | Align::XMinYMin => (0.0, 0.0),
        Align::XMidYMin => (0.5, 0.0),
        Align::XMaxYMin => (1.0, 0.0),
        Align::XMinYMid => (0.0, 0.5),
        Align::XMidYMid => (0.5, 0.5),
        Align::XMaxYMid => (1.0, 0.5),
        Align::XMinYMax => (0.0, 1.0),
        Align::XMidYMax => (0.5, 1.0),
        Align::XMaxYMax => (1.0, 1.0),
    };
    let tx = (width - view_box.width * sx) * align_x - view_box.x * sx;
    let ty = (height - view_box.height * sy) * align_y - view_box.y * sy;
    SvgTransform {
        sx,
        ky: 0.0,
        kx: 0.0,
        sy,
        tx,
        ty,
    }
}
