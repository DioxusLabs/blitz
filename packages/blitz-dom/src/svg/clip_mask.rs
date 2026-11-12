//! `clip-path` and `mask` references to `<clipPath>` and `<mask>` elements.
//!
//! Like paint servers these are paint-only references: the painter resolves
//! them by id for each element it paints, using the layout cached on the
//! resource's children. A reference back to a resource that is already being
//! applied (a cycle) is ignored, as resvg and browsers do.
//! <https://drafts.fxtf.org/css-masking-1/>

use blitz_traits::node_id::NodeId;
use kurbo::{Affine, Rect};
use markup5ever::{LocalName, local_name, ns};
use style::computed_values::fill_rule::T as FillRule;
use style::computed_values::visibility::T as Visibility;
use style::properties::ComputedValues;
use style::values::computed::basic_shape::ClipPath;
use style::values::computed::url::ComputedUrl;
use style::values::generics::image::GenericImage;
use style::values::specified::svg::{MaskMode, MaskType};

use super::geometry::{font_size, resolve_svg_length};
use super::{SvgLayoutKind, SvgViewport, attr};
use crate::BaseDocument;
use crate::node::Node;

/// How an element's `clip-path` or `mask` reference resolves.
#[derive(Debug, Clone)]
pub enum SvgReference<T> {
    /// No reference, or one that is ignored: to another document, to a
    /// missing or `display: none` element, or back into a cycle.
    None,
    /// A reference that disables rendering of the element: to an element of
    /// the wrong type, or with `objectBoundingBox` units and an empty box.
    Invalid,
    Valid(T),
}

/// A resolved `<clipPath>`: the union of its children's shapes.
#[derive(Debug, Clone)]
pub struct SvgClip {
    /// Maps the clip path's content (after `clipPathUnits` and the clip
    /// path's own `transform`) to the referencing element's user space.
    pub transform: Affine,
    pub children: Vec<SvgClipChild>,
    /// The `clip-path` of the `<clipPath>` itself, in the referencing
    /// element's user space.
    pub clip: Option<Box<SvgClip>>,
}

/// A shape or `<text>` child of a `<clipPath>`.
#[derive(Debug, Clone)]
pub struct SvgClipChild {
    pub node: NodeId,
    /// `clip-rule: evenodd`.
    pub evenodd: bool,
    /// The child's own `clip-path`, in its user space.
    pub clip: Option<SvgClip>,
}

/// A resolved `<mask>`.
#[derive(Debug, Clone)]
pub struct SvgMask {
    /// The `<mask>` element, whose children are the mask content.
    pub node: NodeId,
    /// The mask region in the referencing element's user space.
    pub rect: Rect,
    /// Maps the mask content (`maskContentUnits`) to the referencing
    /// element's user space.
    pub content_transform: Affine,
    /// Whether the mask uses its alpha channel rather than its luminance.
    pub alpha: bool,
    /// The `mask` of the `<mask>` itself, applied to its content.
    pub mask: Option<Box<SvgMask>>,
}

/// The transform that maps the unit square onto `bbox`, or `None` when the
/// box is empty and `objectBoundingBox` units cannot be used.
fn bbox_transform(bbox: Option<Rect>) -> Option<Affine> {
    let bbox = bbox.filter(|b| b.width() > 0.0 && b.height() > 0.0)?;
    Some(Affine::new([
        bbox.width(),
        0.0,
        0.0,
        bbox.height(),
        bbox.x0,
        bbox.y0,
    ]))
}

fn units_attr<'a>(node: &'a Node, name: &str) -> Option<&'a str> {
    attr(node.element_data()?, name).map(str::trim)
}

impl BaseDocument {
    /// The resource `url` points at when it is an SVG `tag` element with
    /// cached layout that is not in `active`.
    fn svg_resource(
        &self,
        url: &ComputedUrl,
        tag: LocalName,
        active: &[NodeId],
    ) -> SvgReference<&Node> {
        let Some(id) = url.url().and_then(|url| self.same_document_id(url)) else {
            return SvgReference::None;
        };
        let Some(node) = self.get_element_by_id(&id).and_then(|id| self.get_node(id)) else {
            return SvgReference::None;
        };
        if active.contains(&node.id) {
            return SvgReference::None;
        }
        let Some(element) = node.element_data() else {
            return SvgReference::None;
        };
        if element.name.ns != ns!(svg) || element.name.local != tag {
            return SvgReference::Invalid;
        }
        match element.svg_layout() {
            Some(layout) if matches!(layout.kind, SvgLayoutKind::Resource) => {
                SvgReference::Valid(node)
            }
            _ => SvgReference::None,
        }
    }

    /// Resolve the `clip-path` of an SVG element with object bounding box
    /// `bbox`. `active` holds the clip paths and masks being applied.
    pub fn resolve_svg_clip_path(
        &self,
        style: &ComputedValues,
        bbox: Option<Rect>,
        active: &mut Vec<NodeId>,
    ) -> SvgReference<SvgClip> {
        let ClipPath::Url(url) = &style.get_svg().clip_path else {
            return SvgReference::None;
        };
        let node = match self.svg_resource(url, local_name!("clipPath"), active) {
            SvgReference::Valid(node) => node,
            SvgReference::None => return SvgReference::None,
            SvgReference::Invalid => return SvgReference::Invalid,
        };
        let layout = node.element_data().unwrap().svg_layout().unwrap();
        let mut transform = layout.transform;
        if units_attr(node, "clipPathUnits") == Some("objectBoundingBox") {
            let Some(bbox) = bbox_transform(bbox) else {
                return SvgReference::Invalid;
            };
            transform *= bbox;
        }
        let Some(clip_style) = node.primary_styles() else {
            return SvgReference::None;
        };

        active.push(node.id);
        let result = (|| {
            let clip = match self.resolve_svg_clip_path(&clip_style, bbox, active) {
                SvgReference::None => None,
                SvgReference::Invalid => return SvgReference::Invalid,
                SvgReference::Valid(clip) => Some(Box::new(clip)),
            };
            let mut children = Vec::new();
            for &child in &node.children {
                let child_node = node.with(child);
                let Some(child_layout) = child_node.element_data().and_then(|el| el.svg_layout())
                else {
                    continue;
                };
                if !matches!(
                    child_layout.kind,
                    SvgLayoutKind::Shape { .. } | SvgLayoutKind::Text(_)
                ) {
                    continue;
                }
                let Some(child_style) = child_node.primary_styles() else {
                    continue;
                };
                if child_style.get_inherited_box().visibility != Visibility::Visible {
                    continue;
                }
                let clip = match self.resolve_svg_clip_path(&child_style, child_layout.bbox, active)
                {
                    SvgReference::None => None,
                    SvgReference::Invalid => continue,
                    SvgReference::Valid(clip) => Some(clip),
                };
                children.push(SvgClipChild {
                    node: child,
                    evenodd: child_style.get_inherited_svg().clip_rule == FillRule::Evenodd,
                    clip,
                });
            }
            SvgReference::Valid(SvgClip {
                transform,
                children,
                clip,
            })
        })();
        active.pop();
        result
    }

    /// Resolve the `mask` of an SVG element with object bounding box `bbox`
    /// in `viewport`. `active` holds the clip paths and masks being applied.
    pub fn resolve_svg_mask(
        &self,
        style: &ComputedValues,
        bbox: Option<Rect>,
        viewport: SvgViewport,
        active: &mut Vec<NodeId>,
    ) -> SvgReference<SvgMask> {
        let svg = style.get_svg();
        let Some(GenericImage::Url(url)) = svg.mask_image.0.first() else {
            return SvgReference::None;
        };
        let node = match self.svg_resource(url, local_name!("mask"), active) {
            SvgReference::Valid(node) => node,
            SvgReference::None => return SvgReference::None,
            SvgReference::Invalid => return SvgReference::Invalid,
        };
        let Some(mask_style) = node.primary_styles() else {
            return SvgReference::None;
        };

        // The mask region: `maskUnits` defaults to `objectBoundingBox` and
        // the region to -10%/-10%/120%/120%.
        let length = |name: &str, default: f64| {
            units_attr(node, name)
                .and_then(|value| value.parse::<svgtypes::Length>().ok())
                .unwrap_or(svgtypes::Length::new(
                    default,
                    svgtypes::LengthUnit::Percent,
                ))
        };
        let (x, y) = (length("x", -10.0), length("y", -10.0));
        let (width, height) = (length("width", 120.0), length("height", 120.0));
        let rect = if units_attr(node, "maskUnits") == Some("userSpaceOnUse") {
            let fs = font_size(&mask_style);
            let x = resolve_svg_length(x, viewport.width, fs);
            let y = resolve_svg_length(y, viewport.height, fs);
            let width = resolve_svg_length(width, viewport.width, fs);
            let height = resolve_svg_length(height, viewport.height, fs);
            Rect::new(x, y, x + width, y + height)
        } else {
            let Some(bbox) = bbox_transform(bbox) else {
                return SvgReference::Invalid;
            };
            let fraction = |len: svgtypes::Length| match len.unit {
                svgtypes::LengthUnit::Percent => len.number / 100.0,
                _ => len.number,
            };
            let (x, y) = (fraction(x), fraction(y));
            bbox.transform_rect_bbox(Rect::new(x, y, x + fraction(width), y + fraction(height)))
        };
        if !(rect.width() > 0.0 && rect.height() > 0.0) {
            return SvgReference::Invalid;
        }
        let content_transform = if units_attr(node, "maskContentUnits") == Some("objectBoundingBox")
        {
            let Some(bbox) = bbox_transform(bbox) else {
                return SvgReference::Invalid;
            };
            bbox
        } else {
            Affine::IDENTITY
        };
        let alpha = match svg.mask_mode.0.first() {
            Some(MaskMode::Alpha) => true,
            Some(MaskMode::Luminance) => false,
            _ => mask_style.get_svg().mask_type == MaskType::Alpha,
        };

        active.push(node.id);
        let mask = self.resolve_svg_mask(&mask_style, bbox, viewport, active);
        active.pop();
        let mask = match mask {
            SvgReference::None => None,
            SvgReference::Invalid => return SvgReference::Invalid,
            SvgReference::Valid(mask) => Some(Box::new(mask)),
        };
        SvgReference::Valid(SvgMask {
            node: node.id,
            rect,
            content_transform,
            alpha,
            mask,
        })
    }
}
