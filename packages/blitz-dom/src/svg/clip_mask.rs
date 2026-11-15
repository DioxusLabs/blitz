//! `clip-path` and `mask` references to `<clipPath>` and `<mask>` elements.
//!
//! Like paint servers these are paint-only references: the painter resolves
//! them by id for each element it paints, using the layout cached on the
//! resource's children.
//!
//! References that close a cycle are found once per layout for the whole
//! document and ignored everywhere, like usvg does. How deeply and how many
//! references are followed for one element is bounded, so that long chains
//! and branching graphs of references cannot exhaust the stack or the
//! renderer.
//! <https://drafts.fxtf.org/css-masking-1/>

use std::collections::{HashMap, HashSet};

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
use super::{SvgLayoutKind, SvgViewport, attr, is_resource_container};
use crate::BaseDocument;
use crate::node::Node;

/// How an element's `clip-path` or `mask` reference resolves.
#[derive(Debug, Clone)]
pub enum SvgReference<T> {
    /// No reference, or one that is ignored: to another document, to a
    /// missing or `display: none` element or one of the wrong type, back
    /// into a cycle, or beyond the reference limits.
    None,
    /// A reference that disables rendering of the element: with
    /// `objectBoundingBox` units and an empty box, or an empty mask region.
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

/// How many clip paths and masks can be nested inside each other.
const MAX_DEPTH: usize = 32;
/// How many clip path and mask references are followed for one element
/// (including the content of its masks).
const MAX_RESOLVED: usize = 256;

/// The clip paths and masks being applied while painting, which bound the
/// references that are followed.
#[derive(Debug, Default)]
pub struct SvgReferenceState {
    active: Vec<NodeId>,
    resolved: usize,
}

impl SvgReferenceState {
    /// Called before resolving the references of an element to paint. Each
    /// element outside any clip path or mask gets a new budget.
    pub fn start_element(&mut self) {
        if self.active.is_empty() {
            self.resolved = 0;
        }
    }

    /// Enter the content of the clip path or mask `id`.
    pub fn push(&mut self, id: NodeId) {
        self.active.push(id);
    }

    pub fn pop(&mut self) {
        self.active.pop();
    }
}

fn units_attr<'a>(node: &'a Node, name: &str) -> Option<&'a str> {
    attr(node.element_data()?, name).map(str::trim)
}

impl BaseDocument {
    /// The resource that `user`'s `url` points at when it is an SVG `tag`
    /// element with cached layout, the reference does not close a cycle and
    /// the reference limits allow it.
    fn svg_resource(
        &self,
        url: &ComputedUrl,
        tag: LocalName,
        user: NodeId,
        state: &mut SvgReferenceState,
    ) -> Option<&Node> {
        let is_mask = tag == local_name!("mask");
        if self.svg_circular_references.contains(&(user, is_mask)) {
            return None;
        }
        let id = url.url().and_then(|url| self.same_document_id(url))?;
        let node = self.get_node(self.get_element_by_id(&id)?)?;
        let element = node.element_data()?;
        if element.name.ns != ns!(svg) || element.name.local != tag {
            return None;
        }
        if state.active.contains(&node.id)
            || state.active.len() >= MAX_DEPTH
            || state.resolved >= MAX_RESOLVED
        {
            return None;
        }
        if !matches!(element.svg_layout()?.kind, SvgLayoutKind::Resource) {
            return None;
        }
        state.resolved += 1;
        Some(node)
    }

    /// The `<clipPath>` (or `<mask>`) element that `node`'s `clip-path` (or
    /// `mask`) points at.
    fn svg_reference_target(&self, node: &Node, mask: bool) -> Option<NodeId> {
        let style = node.svg_style()?;
        let svg = style.get_svg();
        let (url, tag) = if mask {
            let Some(GenericImage::Url(url)) = svg.mask_image.0.first() else {
                return None;
            };
            (url, local_name!("mask"))
        } else {
            let ClipPath::Url(url) = &svg.clip_path else {
                return None;
            };
            (url, local_name!("clipPath"))
        };
        let id = url.url().and_then(|url| self.same_document_id(url))?;
        let target = self.get_node(self.get_element_by_id(&id)?)?;
        let element = target.element_data()?;
        (element.name.ns == ns!(svg) && element.name.local == tag).then_some(target.id)
    }

    /// The references followed when applying the `<clipPath>` or `<mask>`
    /// `id`, as (element using the reference, whether it is a mask, target).
    fn svg_resource_references(&self, id: NodeId) -> Vec<(NodeId, bool, NodeId)> {
        let node = &self.nodes[id];
        let is_mask = node
            .element_data()
            .is_some_and(|el| el.name.local == local_name!("mask"));
        let mut references = Vec::new();
        let mut push = |user: &Node, mask: bool| {
            if let Some(target) = self.svg_reference_target(user, mask) {
                references.push((user.id, mask, target));
            }
        };
        push(node, is_mask);
        if is_mask {
            // Everything in the mask content except nested resources.
            let mut stack: Vec<NodeId> = node.children.iter().rev().copied().collect();
            while let Some(id) = stack.pop() {
                let child = &self.nodes[id];
                let Some(element) = child.element_data() else {
                    continue;
                };
                if element.name.ns != ns!(svg) || is_resource_container(&element.name.local) {
                    continue;
                }
                push(child, false);
                push(child, true);
                stack.extend(child.children.iter().rev().copied());
            }
        } else {
            for &id in &node.children {
                if self.nodes[id].is_element() {
                    push(&self.nodes[id], false);
                }
            }
        }
        references
    }

    /// Find the `clip-path` and `mask` references that close a cycle, by a
    /// depth-first search over the `<clipPath>` and `<mask>` elements in
    /// document order (as usvg does).
    pub(crate) fn find_svg_circular_references(&mut self) {
        let mut resources = Vec::new();
        for &root in &self.svg_roots {
            let mut stack = vec![root];
            while let Some(id) = stack.pop() {
                let node = &self.nodes[id];
                if node.element_data().is_some_and(|el| {
                    el.name.ns == ns!(svg)
                        && (el.name.local == local_name!("clipPath")
                            || el.name.local == local_name!("mask"))
                }) {
                    resources.push(id);
                }
                stack.extend(node.children.iter().rev().copied());
            }
        }

        // Whether each visited resource is done (rather than in progress).
        let mut visits: HashMap<NodeId, bool> = HashMap::new();
        let mut circular = HashSet::new();
        for resource in resources {
            if visits.contains_key(&resource) {
                continue;
            }
            visits.insert(resource, false);
            let mut stack = vec![(resource, self.svg_resource_references(resource).into_iter())];
            while let Some((id, references)) = stack.last_mut() {
                match references.next() {
                    Some((user, mask, target)) => match visits.get(&target) {
                        Some(false) => {
                            circular.insert((user, mask));
                        }
                        Some(true) => {}
                        None => {
                            visits.insert(target, false);
                            stack.push((target, self.svg_resource_references(target).into_iter()));
                        }
                    },
                    None => {
                        visits.insert(*id, true);
                        stack.pop();
                    }
                }
            }
        }
        self.svg_circular_references = circular;
    }

    /// Resolve the `clip-path` of the SVG element `user`, with computed style
    /// `style` and object bounding box `bbox`.
    pub fn resolve_svg_clip_path(
        &self,
        user: NodeId,
        style: &ComputedValues,
        bbox: Option<Rect>,
        state: &mut SvgReferenceState,
    ) -> SvgReference<SvgClip> {
        let ClipPath::Url(url) = &style.get_svg().clip_path else {
            return SvgReference::None;
        };
        let Some(node) = self.svg_resource(url, local_name!("clipPath"), user, state) else {
            return SvgReference::None;
        };
        let layout = node.element_data().unwrap().svg_layout().unwrap();
        let mut transform = layout.transform;
        if units_attr(node, "clipPathUnits") == Some("objectBoundingBox") {
            let Some(bbox) = bbox_transform(bbox) else {
                return SvgReference::Invalid;
            };
            transform *= bbox;
        }
        let Some(clip_style) = node.svg_style() else {
            return SvgReference::None;
        };

        state.push(node.id);
        let result = (|| {
            let clip = match self.resolve_svg_clip_path(node.id, &clip_style, bbox, state) {
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
                let Some(child_style) = child_node.svg_style() else {
                    continue;
                };
                if child_style.get_inherited_box().visibility != Visibility::Visible {
                    continue;
                }
                let clip =
                    match self.resolve_svg_clip_path(child, &child_style, child_layout.bbox, state)
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
        state.pop();
        result
    }

    /// Resolve the `mask` of the SVG element `user`, with computed style
    /// `style` and object bounding box `bbox`, in `viewport`.
    pub fn resolve_svg_mask(
        &self,
        user: NodeId,
        style: &ComputedValues,
        bbox: Option<Rect>,
        viewport: SvgViewport,
        state: &mut SvgReferenceState,
    ) -> SvgReference<SvgMask> {
        let svg = style.get_svg();
        let Some(GenericImage::Url(url)) = svg.mask_image.0.first() else {
            return SvgReference::None;
        };
        let Some(node) = self.svg_resource(url, local_name!("mask"), user, state) else {
            return SvgReference::None;
        };
        let Some(mask_style) = node.svg_style() else {
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

        state.push(node.id);
        let mask = self.resolve_svg_mask(node.id, &mask_style, bbox, viewport, state);
        state.pop();
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
