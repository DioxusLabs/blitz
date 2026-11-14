//! Hit testing of natively rendered inline SVG.
//!
//! Pointer events target the individual graphics elements of a native
//! `<svg>`: a point hits a shape when it is inside the fill or the stroke
//! outline that SVG layout computed, as the shape's `pointer-events`,
//! `visibility`, `fill` and `stroke` allow, and a `<text>` through its
//! character cells. Children are tested in reverse paint order.
//! <https://svgwg.org/svg2-draft/interact.html#hit-testing>

use blitz_traits::node_id::NodeId;
use kurbo::{Affine, BezPath, ParamCurveNearest, PathEl, Point, Rect, Shape};
use style::computed_values::fill_rule::T as FillRule;
use style::computed_values::overflow_x::T as Overflow;
use style::computed_values::pointer_events::T as PointerEvents;
use style::computed_values::visibility::T as Visibility;
use style::properties::ComputedValues;
use style::values::computed::svg::SVGPaintKind;

use super::{SvgLayoutKind, SvgText};
use crate::node::Node;

/// How close to an outline, in CSS px, a point counts as on it.
const EDGE_TOLERANCE: f64 = 1e-3;
/// The flattening tolerance of stroke outlines, in CSS px.
const STROKE_TOLERANCE: f64 = 0.05;

impl Node {
    /// The SVG element under a point of a natively rendered `<svg>` root, or
    /// `None` when no element of its content is hit. The point is relative
    /// to the root's border box, in CSS px.
    pub(crate) fn svg_hit(&self, x: f32, y: f32) -> Option<NodeId> {
        let root = self.element_data()?.svg_root_data()?;
        let layout = root.layout.as_ref()?;
        if root.dims.degenerate_view_box {
            return None;
        }
        let l = self.final_layout();
        let content_box = Rect::new(
            (l.border.left + l.padding.left) as f64,
            (l.border.top + l.padding.top) as f64,
            (l.size.width - l.border.right - l.padding.right) as f64,
            (l.size.height - l.border.bottom - l.padding.bottom) as f64,
        );
        if content_box.width() <= 0.0 || content_box.height() <= 0.0 {
            return None;
        }
        let point = Point::new(x as f64, y as f64);
        // Content outside the box is not hit where `overflow` clips it.
        if let Some(style) = self.primary_styles() {
            let box_style = style.get_box();
            if (box_style.overflow_x != Overflow::Visible
                && !(content_box.x0..=content_box.x1).contains(&point.x))
                || (box_style.overflow_y != Overflow::Visible
                    && !(content_box.y0..=content_box.y1).contains(&point.y))
            {
                return None;
            }
        }
        let transform =
            Affine::translate(content_box.origin().to_vec2()) * layout.children_transform();
        hit_children(self, transform, point)
    }
}

// Hit testing can run between a DOM mutation and the next layout, so ids
// cached in the layout may name removed nodes: skip those.
fn hit_children(node: &Node, transform: Affine, point: Point) -> Option<NodeId> {
    node.children
        .iter()
        .rev()
        .filter_map(|child| node.tree().get(*child))
        .find_map(|child| hit_node(child, transform, point))
}

/// `parent_transform` maps the parent's user space to CSS px of the root's
/// border box, where `point` is.
fn hit_node(node: &Node, parent_transform: Affine, point: Point) -> Option<NodeId> {
    let layout = node.element_data()?.svg_layout()?;
    let transform = parent_transform * layout.transform;
    match &layout.kind {
        SvgLayoutKind::NotRendered | SvgLayoutKind::Resource => None,
        SvgLayoutKind::Group => hit_children(node, transform, point),
        SvgLayoutKind::Switch { child } => {
            let child = node.tree().get((*child)?)?;
            hit_node(child, transform, point)
        }
        SvgLayoutKind::Viewport {
            content_transform,
            clip,
            ..
        } => {
            if let Some(clip) = clip
                && !invert(transform, point).is_some_and(|p| contains_inclusive(*clip, p))
            {
                return None;
            }
            hit_children(node, transform * *content_transform, point)
        }
        SvgLayoutKind::Shape { path, stroke } => {
            let style = node.primary_styles()?;
            let (fill_area, stroke_area) = hit_areas(&style);
            let local = invert(transform, point)?;
            // Tolerances are given in CSS px and applied in user space.
            let scale = transform.determinant().abs().sqrt();
            let tolerance = EDGE_TOLERANCE / scale;
            let bbox = layout.bbox?;
            let fill_hit =
                fill_area && contains_inclusive(bbox.inflate(tolerance, tolerance), local) && {
                    let nonzero = style.get_inherited_svg().fill_rule == FillRule::Nonzero;
                    fill_contains(path, local, nonzero, tolerance)
                };
            let stroke_hit = !fill_hit
                && stroke_area
                && stroke.as_ref().is_some_and(|stroke| {
                    let reach = stroke_reach(stroke) + tolerance;
                    if !contains_inclusive(bbox.inflate(reach, reach), local) {
                        return false;
                    }
                    let outline = kurbo::stroke(
                        path.iter(),
                        stroke,
                        &kurbo::StrokeOpts::default(),
                        STROKE_TOLERANCE / scale,
                    );
                    let outline_bbox = outline.bounding_box().inflate(tolerance, tolerance);
                    contains_inclusive(outline_bbox, local)
                        && fill_contains(&outline, local, true, tolerance)
                });
            (fill_hit || stroke_hit).then_some(node.id)
        }
        SvgLayoutKind::Text(text) => hit_text(node, text, transform, point),
    }
}

/// The `<text>` or `<tspan>` whose character cell contains the point. A
/// cell is both the fill and the stroke area of its character.
fn hit_text(node: &Node, text: &SvgText, transform: Affine, point: Point) -> Option<NodeId> {
    let bbox = text.bbox?;
    if !invert(transform, point).is_some_and(|p| contains_inclusive(bbox, p)) {
        return None;
    }
    text.cells.iter().rev().find_map(|cell| {
        let style = node.tree().get(cell.node)?.primary_styles()?;
        let (fill_area, stroke_area) = hit_areas(&style);
        if !fill_area && !stroke_area {
            return None;
        }
        let local = invert(transform * cell.transform, point)?;
        contains_inclusive(cell.rect, local).then_some(cell.node)
    })
}

/// Whether an element's fill and its stroke can be hit, from its
/// `pointer-events`, `visibility`, `fill` and `stroke`.
/// <https://svgwg.org/svg2-draft/interact.html#PointerEventsProperty>
fn hit_areas(style: &ComputedValues) -> (bool, bool) {
    let visible = style.get_inherited_box().visibility == Visibility::Visible;
    let svg = style.get_inherited_svg();
    let fill_painted = !matches!(svg.fill.kind, SVGPaintKind::None);
    let stroke_painted = !matches!(svg.stroke.kind, SVGPaintKind::None);
    match style.get_inherited_ui().pointer_events {
        PointerEvents::Auto | PointerEvents::Visiblepainted => {
            (visible && fill_painted, visible && stroke_painted)
        }
        PointerEvents::Visiblefill => (visible, false),
        PointerEvents::Visiblestroke => (false, visible),
        PointerEvents::Visible => (visible, visible),
        PointerEvents::Painted => (fill_painted, stroke_painted),
        PointerEvents::Fill => (true, false),
        PointerEvents::Stroke => (false, true),
        PointerEvents::All => (true, true),
        PointerEvents::None => (false, false),
    }
}

/// How far from the path the stroke outline can reach: half the stroke
/// width, scaled by the longest miter or square cap.
fn stroke_reach(stroke: &kurbo::Stroke) -> f64 {
    let join = match stroke.join {
        kurbo::Join::Miter => stroke.miter_limit.max(1.0),
        _ => 1.0,
    };
    let cap = match (stroke.start_cap, stroke.end_cap) {
        (kurbo::Cap::Square, _) | (_, kurbo::Cap::Square) => std::f64::consts::SQRT_2,
        _ => 1.0,
    };
    stroke.width / 2.0 * join.max(cap)
}

fn invert(transform: Affine, point: Point) -> Option<Point> {
    let det = transform.determinant();
    (det.is_finite() && det.abs() > 1e-12).then(|| transform.inverse() * point)
}

fn contains_inclusive(rect: Rect, point: Point) -> bool {
    point.x >= rect.x0 && point.x <= rect.x1 && point.y >= rect.y0 && point.y <= rect.y1
}

/// Whether the fill of `path` contains `point`, with open subpaths closed
/// as filling does and points within `tolerance` of an edge counted inside.
/// Callers reject points outside the path's bounds first.
fn fill_contains(path: &BezPath, point: Point, nonzero: bool, tolerance: f64) -> bool {
    let closed = close_subpaths(path);
    let winding = closed.winding(point);
    let inside = if nonzero {
        winding != 0
    } else {
        winding % 2 != 0
    };
    inside
        || closed
            .segments()
            .any(|seg| seg.nearest(point, 1e-6).distance_sq <= tolerance * tolerance)
}

/// `path` with every open subpath closed.
fn close_subpaths(path: &BezPath) -> BezPath {
    let mut closed = BezPath::new();
    let mut open = false;
    for el in path.elements() {
        match el {
            PathEl::MoveTo(_) => {
                if open {
                    closed.close_path();
                }
                open = true;
            }
            PathEl::ClosePath => open = false,
            _ => {}
        }
        closed.push(*el);
    }
    if open {
        closed.close_path();
    }
    closed
}
