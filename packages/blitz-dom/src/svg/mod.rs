//! Native inline SVG layout.
//!
//! A natively rendered inline `<svg>` (`svg-native` feature) is a
//! replaced box for Taffy; its descendants get no boxes. After Taffy has sized
//! the root, `BaseDocument::resolve_svg_layout` walks the damaged parts of
//! each root's subtree and stores on every rendered SVG element its local
//! transform, its geometry (a [`BezPath`] and [`Stroke`] in user units) and
//! its object bounding box. The painter reads these results instead of
//! recomputing geometry every frame.

mod clip_mask;
mod geometry;
mod hit;
mod paint_server;
mod text;

pub use clip_mask::{SvgClip, SvgClipChild, SvgMask, SvgReference, SvgReferenceState};
pub use paint_server::{
    SvgGradient, SvgGradientShape, SvgGradientStop, SvgPaintServer, SvgSpreadMethod,
};
pub use text::{
    SvgGlyph, SvgGlyphRun, SvgText, SvgTextCell, SvgTextDecoration, SvgTextDecorationKind,
};

use blitz_traits::node_id::NodeId;
use kurbo::{Affine, BezPath, Rect, Stroke};
use markup5ever::{local_name, ns};
use style::computed_values::mix_blend_mode::T as MixBlendMode;
use style::computed_values::overflow_x::T as Overflow;
use style::properties::ComputedValues;
use style::servo_arc::Arc as ServoArc;
use style::values::computed::svg::SVGPaintKind;
use style::values::computed::{Length, LengthPercentage};

use crate::node::{ElementData, Node, SpecialElementData, SvgIntrinsicDimensions};
use crate::{BaseDocument, CopyStyleGetters, resolve_2d_transform};

/// The SVG viewport that percentages resolve against, in user units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvgViewport {
    pub width: f64,
    pub height: f64,
    /// The effective `zoom` of the element whose lengths are resolved:
    /// computed lengths include it, user units do not.
    pub zoom: f64,
}

impl SvgViewport {
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            width,
            height,
            zoom: 1.0,
        }
    }
    fn for_element(self, style: &ComputedValues) -> Self {
        Self {
            zoom: style.effective_zoom.value() as f64,
            ..self
        }
    }
    /// The normalized diagonal, the basis for percentages that are neither
    /// horizontal nor vertical. <https://svgwg.org/svg2-draft/coords.html#Units>
    pub fn diagonal(&self) -> f64 {
        ((self.width * self.width + self.height * self.height) / 2.0).sqrt()
    }
    pub fn x(&self, lp: &LengthPercentage) -> f64 {
        self.resolve(lp, self.width)
    }
    pub fn y(&self, lp: &LengthPercentage) -> f64 {
        self.resolve(lp, self.height)
    }
    pub fn other(&self, lp: &LengthPercentage) -> f64 {
        self.resolve(lp, self.diagonal())
    }
    /// Resolve a computed length against `basis` (in user units).
    pub fn resolve(&self, lp: &LengthPercentage, basis: f64) -> f64 {
        lp.resolve(Length::new((basis * self.zoom) as f32)).px() as f64 / self.zoom
    }
    /// Remove the zoom from a computed length.
    pub fn unzoom(&self, px: f64) -> f64 {
        px / self.zoom
    }
}

/// The result of SVG layout for one element.
#[derive(Debug, Clone)]
pub struct SvgLayout {
    /// Maps this element's coordinates to its parent's user space (the
    /// `transform` property). For an `<svg>` it is applied before the
    /// viewport's own `x`/`y` and `viewBox` mapping (`content_transform`).
    pub transform: Affine,
    /// The object bounding box in this element's user space (for an `<svg>`,
    /// the space of its children). `None` when nothing contributes.
    pub bbox: Option<Rect>,
    /// A conservative bound of what this element paints, in the same space
    /// as `bbox`, ignoring clip paths and masks (which only take away from
    /// it). `None` when it paints nothing.
    pub ink: Option<Rect>,
    pub kind: SvgLayoutKind,
    /// Whether this element or a rendered descendant has a non-normal
    /// `mix-blend-mode`, so the outermost `<svg>` must isolate its content.
    pub has_blending: bool,
    /// The computed style of an element inside a `display: none` subtree
    /// (which Stylo does not style) that is laid out as part of a
    /// `<clipPath>` or `<mask>`, as those can still be referenced.
    undisplayed_style: Option<ServoArc<ComputedValues>>,
}

#[derive(Debug, Clone)]
pub enum SvgLayoutKind {
    /// Not rendered: `display: none`, empty shapes and unsupported elements.
    NotRendered,
    /// Resource containers (`<defs>`, `<clipPath>`, `<mask>`, `<symbol>`,
    /// ...) and elements whose conditional processing attributes fail:
    /// never rendered directly, but their children are laid out so that
    /// references to `<clipPath>` and `<mask>` elements inside can use it.
    Resource,
    /// `<g>` and `<a>`: render all children.
    Group,
    /// `<switch>`: render only the first child whose conditions pass.
    Switch { child: Option<NodeId> },
    /// `<svg>`: a new viewport for the children.
    Viewport {
        /// Maps the children's user space onto this element's coordinates.
        content_transform: Affine,
        /// The viewport children's percentages resolve against.
        viewport: SvgViewport,
        /// The viewport clip in this element's coordinates, when `overflow`
        /// clips. Always `None` on the outermost `<svg>`, whose CSS box clips.
        clip: Option<Rect>,
    },
    /// A basic shape or `<path>`: its outline in user space and, when it has
    /// a stroke, the resolved stroke style (`pathLength` already applied).
    Shape {
        path: BezPath,
        stroke: Option<Stroke>,
    },
    /// `<text>`: its shaped and positioned glyphs. The content of its
    /// `<tspan>` descendants is laid out with it.
    Text(Box<SvgText>),
}

impl SvgLayout {
    fn not_rendered() -> Self {
        Self {
            transform: Affine::IDENTITY,
            bbox: None,
            ink: None,
            kind: SvgLayoutKind::NotRendered,
            has_blending: false,
            undisplayed_style: None,
        }
    }

    /// The transform from this element's children's user space to the
    /// parent's user space.
    pub fn children_transform(&self) -> Affine {
        match &self.kind {
            SvgLayoutKind::Viewport {
                content_transform, ..
            } => self.transform * *content_transform,
            _ => self.transform,
        }
    }

    /// [`Self::ink`] in the parent's user space, clipped by a nested
    /// `<svg>`'s viewport clip.
    pub fn ink_in_parent(&self) -> Option<Rect> {
        let ink = self.ink?;
        match &self.kind {
            SvgLayoutKind::NotRendered | SvgLayoutKind::Resource => None,
            SvgLayoutKind::Viewport {
                content_transform,
                clip,
                ..
            } => {
                let ink = content_transform.transform_rect_bbox(ink);
                let ink = clip.map_or(ink, |clip| ink.intersect(clip));
                Some(self.transform.transform_rect_bbox(ink))
            }
            _ => Some(self.transform.transform_rect_bbox(ink)),
        }
    }

    /// The bounding box in the parent's user space.
    fn bbox_in_parent(&self) -> Option<Rect> {
        if matches!(self.kind, SvgLayoutKind::Resource) {
            return None;
        }
        if let SvgLayoutKind::Shape { path, .. } = &self.kind {
            if self.transform == Affine::IDENTITY {
                return self.bbox;
            }
            return Some(kurbo::Shape::bounding_box(&(self.transform * path.clone())));
        }
        let bbox = self.bbox?;
        Some(self.children_transform().transform_rect_bbox(bbox))
    }
}

/// Data for the root of a natively rendered inline `<svg>` subtree.
#[derive(Debug, Clone)]
pub struct SvgRootData {
    /// The dimensions declared on the element (`width`, `height`, `viewBox`,
    /// `preserveAspectRatio`).
    pub dims: SvgIntrinsicDimensions,
    /// The root's layout, with `content_transform` from the content box (in
    /// CSS px) to the root user space. `None` before the first layout pass.
    pub layout: Option<SvgLayout>,
}

impl SvgRootData {
    pub fn new(dims: SvgIntrinsicDimensions) -> Self {
        Self { dims, layout: None }
    }
}

impl Node {
    /// The computed style of an SVG element: its primary style or, inside a
    /// `display: none` subtree, the one resolved for SVG layout.
    pub fn svg_style(&self) -> Option<ServoArc<ComputedValues>> {
        if let Some(style) = self.primary_styles() {
            return Some(style.clone());
        }
        self.element_data()?.svg_layout()?.undisplayed_style.clone()
    }
}

/// Convert an SVG transform into a kurbo one.
pub(crate) fn svg_to_affine(t: crate::node::SvgTransform) -> Affine {
    Affine::new([t.sx, t.ky, t.kx, t.sy, t.tx, t.ty].map(f64::from))
}

pub(crate) fn attr<'a>(element: &'a ElementData, name: &str) -> Option<&'a str> {
    element
        .attrs()
        .iter()
        .find(|a| &*a.name.local == name)
        .map(|a| a.value.as_str())
}

/// Elements whose content is never rendered directly, only through a
/// reference (`<use>`, `fill="url(#id)"`, `clip-path`, ...). They are kept
/// styled (not `display: none`) so that their children have computed styles.
pub fn is_resource_container(tag: &markup5ever::LocalName) -> bool {
    *tag == local_name!("defs")
        || *tag == local_name!("clipPath")
        || *tag == local_name!("mask")
        || *tag == local_name!("marker")
        || *tag == local_name!("pattern")
        || *tag == local_name!("symbol")
        || *tag == local_name!("filter")
        || *tag == local_name!("linearGradient")
        || *tag == local_name!("radialGradient")
}

/// The language conditional processing matches `systemLanguage` against.
const USER_LANGUAGE: &str = "en";

/// Whether the element's conditional processing attributes evaluate to true.
/// <https://svgwg.org/svg2-draft/struct.html#ConditionalProcessing>
fn conditions_pass(element: &ElementData) -> bool {
    // No extensions are supported, so any `requiredExtensions` (including the
    // empty string) evaluates to false.
    if attr(element, "requiredExtensions").is_some() {
        return false;
    }
    if let Some(languages) = attr(element, "systemLanguage") {
        return languages.split(',').map(str::trim).any(|lang| {
            lang.eq_ignore_ascii_case(USER_LANGUAGE)
                || (lang.len() > USER_LANGUAGE.len()
                    && lang.is_char_boundary(USER_LANGUAGE.len())
                    && lang[..USER_LANGUAGE.len()].eq_ignore_ascii_case(USER_LANGUAGE)
                    && lang.as_bytes()[USER_LANGUAGE.len()] == b'-')
        });
    }
    true
}

fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.union(b)),
        (a, b) => a.or(b),
    }
}

impl BaseDocument {
    /// Run SVG layout for every natively rendered `<svg>` root whose subtree
    /// is damaged or whose size changed. Runs after Taffy layout and before
    /// damage is cleared.
    pub(crate) fn resolve_svg_layout(&mut self) {
        if self.svg_roots.is_empty() {
            return;
        }
        let mut roots = std::mem::take(&mut self.svg_roots);
        roots.retain(|id| {
            self.nodes
                .get(*id)
                .and_then(|node| node.element_data())
                .is_some_and(|el| el.svg_root_data().is_some())
        });
        let mut laid_out = false;
        for &root in &roots {
            if self.nodes[root].flags.is_in_document() {
                laid_out |= self.layout_svg_root(root);
            }
        }
        self.svg_roots = roots;
        if laid_out {
            self.find_svg_circular_references();
        }
    }

    /// Lay out a root's damaged descendants, returning whether there were any.
    fn layout_svg_root(&mut self, root_id: NodeId) -> bool {
        let node = &self.nodes[root_id];
        let layout = node.final_layout();
        let width = (layout.size.width
            - layout.border.left
            - layout.border.right
            - layout.padding.left
            - layout.padding.right)
            .max(0.0);
        let height = (layout.size.height
            - layout.border.top
            - layout.border.bottom
            - layout.padding.top
            - layout.padding.bottom)
            .max(0.0);
        let Some(data) = node.element_data().and_then(|el| el.svg_root_data()) else {
            return false;
        };
        let dims = data.dims;
        // Without a viewBox, a user unit is a CSS px before zoom.
        let zoom = node
            .primary_styles()
            .map_or(1.0, |style| style.effective_zoom.value() as f64);
        let (viewport, content_transform) = match dims.view_box {
            Some(vb) => (
                SvgViewport::new(vb.width as f64, vb.height as f64),
                svg_to_affine(dims.viewport_transform(width, height)),
            ),
            None => (
                SvgViewport::new(width as f64 / zoom, height as f64 / zoom),
                Affine::scale(zoom),
            ),
        };
        // Descendants are all laid out again only when the root viewport
        // changes; otherwise only damaged ones are. The root's own damage
        // does not matter: its style reaches descendants through their own
        // restyle damage, and its attributes rebuild `SvgRootData`.
        let force = !matches!(
            &data.layout,
            Some(SvgLayout {
                kind: SvgLayoutKind::Viewport {
                    content_transform: old_transform,
                    viewport: old_viewport,
                    ..
                },
                ..
            }) if *old_transform == content_transform && *old_viewport == viewport
        );
        if !force && !node.has_damaged_descendants() {
            return false;
        }
        let (bbox, has_blending) = self.layout_svg_children(root_id, viewport, force);
        let ink = self.svg_children_ink(root_id);

        let root_layout = SvgLayout {
            transform: Affine::IDENTITY,
            bbox,
            ink,
            kind: SvgLayoutKind::Viewport {
                content_transform,
                viewport,
                clip: None,
            },
            has_blending,
            undisplayed_style: None,
        };
        if let Some(SpecialElementData::SvgRoot(data)) = self.nodes[root_id]
            .element_data_mut()
            .map(|el| &mut el.special_data)
        {
            data.layout = Some(root_layout);
        }
        true
    }

    /// Lay out the children of `parent_id` and return the union of their
    /// bounding boxes in `parent_id`'s children's user space, and whether
    /// any of them has blending.
    fn layout_svg_children(
        &mut self,
        parent_id: NodeId,
        viewport: SvgViewport,
        force: bool,
    ) -> (Option<Rect>, bool) {
        let children = self.nodes[parent_id].children.clone();
        let mut bbox = None;
        let mut has_blending = false;
        for child in children {
            let (child_bbox, child_blending) = self.layout_svg_node(child, viewport, force);
            bbox = union(bbox, child_bbox);
            has_blending |= child_blending;
        }
        (bbox, has_blending)
    }

    /// Lay out one SVG element (if it or a descendant is damaged, or `force`)
    /// and return its bounding box in the parent's user space and whether it
    /// or a descendant has blending.
    ///
    /// Inside a native `<svg>` a node's damage is its own (see
    /// `propagate_damage_flags`); damaged descendants are found through the
    /// damaged-descendants flag.
    fn layout_svg_node(
        &mut self,
        id: NodeId,
        viewport: SvgViewport,
        force: bool,
    ) -> (Option<Rect>, bool) {
        let node = &self.nodes[id];
        let Some(element) = node.element_data() else {
            return (None, false);
        };
        if element.name.ns != ns!(svg) {
            return (None, false);
        }
        let existing = element.svg_layout();
        let dirty = force || existing.is_none() || node.damage().is_some_and(|d| !d.is_empty());
        if !dirty && !node.has_damaged_descendants() {
            return existing.map_or((None, false), |l| (l.bbox_in_parent(), l.has_blending));
        }

        let was_rendered = existing.is_some_and(|l| !matches!(l.kind, SvgLayoutKind::NotRendered));
        let style = node.svg_style();
        let mut layout = if dirty {
            let mut layout = match &style {
                Some(style) => layout_own(element, style, viewport),
                None => SvgLayout::not_rendered(),
            };
            if node.primary_styles().is_none() {
                layout.undisplayed_style = style.clone();
            }
            layout
        } else {
            existing.cloned().unwrap()
        };
        let undisplayed = dirty && style.is_some_and(|style| style.clone_display().is_none());

        // Children are laid out again from scratch when this element's
        // viewport may have changed, or when it was not rendered before (its
        // descendants' damage was cleared without being laid out).
        let force_children = force
            || (dirty && (!was_rendered || matches!(layout.kind, SvgLayoutKind::Viewport { .. })));
        let mut children_blending = false;
        match &mut layout.kind {
            SvgLayoutKind::NotRendered if undisplayed => {
                self.layout_undisplayed_resources(id, viewport);
            }
            SvgLayoutKind::NotRendered | SvgLayoutKind::Shape { .. } => {}
            // Text is laid out again whenever it or its content is damaged.
            SvgLayoutKind::Text(text) => {
                let style = self.nodes[id].svg_style().unwrap();
                let viewport = viewport.for_element(&style);
                **text = self.layout_svg_text(id, viewport);
                layout.bbox = text.bbox;
            }
            SvgLayoutKind::Group => {
                (layout.bbox, children_blending) =
                    self.layout_svg_children(id, viewport, force_children);
            }
            // Blending inside a resource is isolated by the clip or mask
            // layer it is painted into.
            SvgLayoutKind::Resource => {
                (layout.bbox, _) = self.layout_svg_children(id, viewport, force_children);
            }
            SvgLayoutKind::Viewport { viewport, .. } => {
                let viewport = *viewport;
                (layout.bbox, children_blending) =
                    self.layout_svg_children(id, viewport, force_children);
            }
            SvgLayoutKind::Switch { child } => {
                let children = self.nodes[id].children.clone();
                *child = children.iter().copied().find(|&c| {
                    self.nodes[c]
                        .element_data()
                        .is_some_and(|el| el.name.ns == ns!(svg) && conditions_pass(el))
                });
                let mut bbox = None;
                for c in children {
                    let (child_bbox, child_blending) =
                        self.layout_svg_node(c, viewport, force_children);
                    if Some(c) == *child {
                        bbox = child_bbox;
                        children_blending = child_blending;
                    }
                }
                layout.bbox = bbox;
            }
        }
        layout.ink = match &layout.kind {
            SvgLayoutKind::NotRendered | SvgLayoutKind::Resource => None,
            SvgLayoutKind::Shape { path, stroke } => {
                let stroked = self.nodes[id].svg_style().is_some_and(|style| {
                    style.get_inherited_svg().stroke.kind != SVGPaintKind::None
                });
                let outset = stroke
                    .as_ref()
                    .filter(|_| stroked)
                    .map_or(0.0, geometry::stroke_outset);
                Some(kurbo::Shape::bounding_box(path).inflate(outset, outset))
            }
            SvgLayoutKind::Text(text) => text.ink,
            SvgLayoutKind::Switch { child } => child.and_then(|child| {
                self.nodes[child]
                    .element_data()
                    .and_then(|el| el.svg_layout())
                    .and_then(SvgLayout::ink_in_parent)
            }),
            SvgLayoutKind::Group | SvgLayoutKind::Viewport { .. } => self.svg_children_ink(id),
        };
        layout.has_blending = !matches!(
            layout.kind,
            SvgLayoutKind::NotRendered | SvgLayoutKind::Resource
        ) && (children_blending
            || self.nodes[id]
                .svg_style()
                .is_some_and(|style| style.get_effects().mix_blend_mode != MixBlendMode::Normal));

        let result = (layout.bbox_in_parent(), layout.has_blending);
        let element = self.nodes[id].element_data_mut().unwrap();
        if matches!(
            element.special_data,
            SpecialElementData::None | SpecialElementData::SvgNode(_)
        ) {
            element.special_data = SpecialElementData::SvgNode(Box::new(layout));
        }
        result
    }

    /// The union of the ink of the children of `id`, in its children's user
    /// space.
    fn svg_children_ink(&self, id: NodeId) -> Option<Rect> {
        self.nodes[id]
            .children
            .iter()
            .filter_map(|&child| {
                self.nodes[child]
                    .element_data()?
                    .svg_layout()?
                    .ink_in_parent()
            })
            .reduce(|a, b| a.union(b))
    }

    /// Lay out the `<clipPath>` and `<mask>` elements inside `id`, a
    /// `display: none` element, as they can still be referenced. Stylo does
    /// not style their subtrees, so their styles are resolved here.
    fn layout_undisplayed_resources(&mut self, id: NodeId, viewport: SvgViewport) {
        let mut stack = self.nodes[id].children.clone();
        while let Some(child) = stack.pop() {
            let node = &self.nodes[child];
            let Some(element) = node.element_data() else {
                continue;
            };
            let tag = &element.name.local;
            if element.name.ns == ns!(svg)
                && (*tag == local_name!("clipPath") || *tag == local_name!("mask"))
            {
                self.resolve_undisplayed_svg_styles(child);
                self.layout_svg_node(child, viewport, true);
            } else {
                stack.extend(node.children.iter().copied());
            }
        }
    }

    /// Store the computed styles of the SVG elements in the subtree of `id`,
    /// which is inside a `display: none` subtree, for SVG layout and paint.
    fn resolve_undisplayed_svg_styles(&mut self, id: NodeId) {
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            let node = &self.nodes[id];
            stack.extend(node.children.iter().copied());
            if node.element_data().is_none_or(|el| el.name.ns != ns!(svg)) {
                continue;
            }
            let style = self.resolve_undisplayed_style(id);
            let element = self.nodes[id].element_data_mut().unwrap();
            match &mut element.special_data {
                SpecialElementData::SvgNode(layout) => layout.undisplayed_style = style,
                data @ SpecialElementData::None => {
                    *data = SpecialElementData::SvgNode(Box::new(SvgLayout {
                        undisplayed_style: style,
                        ..SvgLayout::not_rendered()
                    }));
                }
                _ => {}
            }
        }
    }
}

/// Lay out an element itself (not its children).
fn layout_own(element: &ElementData, style: &ComputedValues, viewport: SvgViewport) -> SvgLayout {
    let viewport = viewport.for_element(style);
    let tag = &element.name.local;
    if style.clone_display().is_none() {
        return SvgLayout::not_rendered();
    }
    let resource = is_resource_container(tag) || !conditions_pass(element);
    // SVG content transforms about the user-space origin (the UA stylesheet
    // sets `transform-origin: 0 0`), so the reference box is empty.
    let transform =
        resolve_2d_transform(style.get_box(), euclid::default::Rect::zero()).unwrap_or_default();

    let kind = if resource {
        SvgLayoutKind::Resource
    } else if *tag == local_name!("g") || *tag == local_name!("a") {
        SvgLayoutKind::Group
    } else if *tag == local_name!("switch") {
        SvgLayoutKind::Switch { child: None }
    } else if *tag == local_name!("text") {
        SvgLayoutKind::Text(Box::default())
    } else if *tag == local_name!("svg") {
        nested_viewport(element, style, viewport)
    } else if let Some(path) = geometry::shape_path(tag, element, style, viewport) {
        let stroke = geometry::stroke(element, style, viewport, &path);
        SvgLayoutKind::Shape { path, stroke }
    } else {
        SvgLayoutKind::NotRendered
    };

    let bbox = match &kind {
        SvgLayoutKind::Shape { path, .. } => Some(kurbo::Shape::bounding_box(path)),
        _ => None,
    };
    SvgLayout {
        transform,
        bbox,
        ink: None,
        kind,
        has_blending: false,
        undisplayed_style: None,
    }
}

/// A nested `<svg>` establishes a new viewport at its `x`/`y`, sized by its
/// `width`/`height` attributes (defaulting to 100%), with its own `viewBox`.
/// The `width`/`height` properties do not apply to it
/// (<https://github.com/w3c/svgwg/pull/1059>).
fn nested_viewport(
    element: &ElementData,
    style: &ComputedValues,
    viewport: SvgViewport,
) -> SvgLayoutKind {
    let x = viewport.x(&style.get_svg().x);
    let y = viewport.y(&style.get_svg().y);
    let dims = SvgIntrinsicDimensions::from_attrs(|name| attr(element, name));
    let font_size = viewport.unzoom(geometry::font_size(style));
    let size = |len: Option<svgtypes::Length>, basis: f64| {
        len.map_or(basis, |len| {
            geometry::resolve_svg_length(len, basis, font_size)
        })
    };
    let width = size(dims.width, viewport.width);
    let height = size(dims.height, viewport.height);
    if width <= 0.0 || height <= 0.0 || dims.degenerate_view_box {
        return SvgLayoutKind::NotRendered;
    }
    let inner = match dims.view_box {
        Some(vb) => SvgViewport::new(vb.width as f64, vb.height as f64),
        None => SvgViewport::new(width, height),
    };
    let content_transform = Affine::translate((x, y))
        * svg_to_affine(dims.viewport_transform(width as f32, height as f32));
    // Any overflow value other than `visible` clips, `auto` included
    // (svgwg#1072, WPT svg/painting/inner-outer-svg-overflow-auto.html);
    // each axis clips separately.
    const UNBOUNDED: f64 = 1e9;
    let box_style = style.get_box();
    let clip_x = box_style.overflow_x != Overflow::Visible;
    let clip_y = box_style.overflow_y != Overflow::Visible;
    let (x0, x1) = if clip_x {
        (x, x + width)
    } else {
        (-UNBOUNDED, UNBOUNDED)
    };
    let (y0, y1) = if clip_y {
        (y, y + height)
    } else {
        (-UNBOUNDED, UNBOUNDED)
    };
    SvgLayoutKind::Viewport {
        content_transform,
        viewport: inner,
        clip: (clip_x || clip_y).then(|| Rect::new(x0, y0, x1, y1)),
    }
}
