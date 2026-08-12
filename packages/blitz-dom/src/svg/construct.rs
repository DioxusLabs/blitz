//! Build (and rebuild, on `CONSTRUCT_SVG` damage) the [`SvgContext`] for
//! every `<svg>` root fragment in the document.
//!
//! Runs *after* Taffy layout (`resolve::resolve_layout`), not during box
//! construction: an inline `<svg>` root is a normal CSS box whose
//! content-box size, needed to resolve `viewBox`/percentage geometry,
//! is only known once Taffy has computed `final_layout` for it. Descendants
//! never get a Taffy node at all, so there is nothing for Taffy layout
//! to do inside the fragment; this pass *is* their layout.

use std::collections::HashMap;

use blitz_traits::node_id::NodeId;
use kurbo::{Affine, Rect, Shape, Size};
use parley::{FontContext, LayoutContext};
use style::Atom;
use style::values::specified::box_::{DisplayInside, DisplayOutside};

use crate::BaseDocument;
use crate::layout::damage::CONSTRUCT_SVG;
use crate::node::{Attribute, SpecialElementData, TextBrush};

use super::attrs::raw_attr;
use super::context::{SvgContext, SvgNode, SvgNodeKind};
use super::geometry;
use super::resolve::{self, MAX_INSTANCED_NODES};
use super::text;
use super::viewport;

/// Elements that establish a non-rendered reference container (UA sheet
/// `display: none`): never walked into the render-order
/// `nodes` list directly, only reachable via `id_map`.
fn is_non_rendered_container(tag: &str) -> bool {
    matches!(
        tag,
        "defs"
            | "clipPath"
            | "marker"
            | "mask"
            | "pattern"
            | "symbol"
            | "filter"
            | "linearGradient"
            | "radialGradient"
            | "stop"
            | "title"
            | "desc"
            | "metadata"
            | "style"
            | "script"
    )
}

/// Rebuild every `SvgRoot` fragment in `doc` that either has no
/// `SvgContext` yet or carries `CONSTRUCT_SVG` damage (sibling `<svg>`
/// roots are left untouched). Called once per layout pass, after Taffy
/// layout completes.
pub fn rebuild_svg_fragments(doc: &mut BaseDocument) {
    let mut roots: Vec<NodeId> = Vec::new();
    for (id, node) in doc.nodes.iter() {
        let Some(elem) = node.data.downcast_element() else {
            continue;
        };
        if matches!(elem.special_data, SpecialElementData::SvgRoot(_)) {
            let needs_rebuild = node
                .damage()
                .unwrap_or(CONSTRUCT_SVG)
                .contains(CONSTRUCT_SVG);
            if needs_rebuild {
                roots.push(id);
            }
        }
    }

    for root in roots {
        let ctx = construct_svg_fragment(doc, root);
        if let Some(elem) = doc.nodes[root].element_data_mut() {
            elem.special_data = SpecialElementData::SvgRoot(std::sync::Arc::new(ctx));
        }
        doc.nodes[root].remove_damage(CONSTRUCT_SVG);
    }
}

/// Build a complete [`SvgContext`] for the `<svg>` root at `root` from
/// scratch (infallible -- always returns a usable, possibly-empty,
/// context; never panics on malformed author input).
pub fn construct_svg_fragment(doc: &BaseDocument, root: NodeId) -> SvgContext {
    let id_map = resolve::build_id_map(doc, root);

    let layout = *doc.nodes[root].final_layout();
    let content_w = (layout.size.width
        - layout.padding.left
        - layout.padding.right
        - layout.border.left
        - layout.border.right)
        .max(0.0) as f64;
    let content_h = (layout.size.height
        - layout.padding.top
        - layout.padding.bottom
        - layout.border.top
        - layout.border.bottom)
        .max(0.0) as f64;
    let viewport = Size::new(content_w, content_h);

    let root_attrs: &[Attribute] = doc.nodes[root].attrs().unwrap_or(&[]);
    let viewbox = raw_attr(root_attrs, "viewBox").and_then(viewport::parse_viewbox);
    let par = raw_attr(root_attrs, "preserveAspectRatio")
        .map(viewport::parse_preserve_aspect_ratio)
        .unwrap_or_default();

    // Display:none/contents on the root -> no fragment at all.
    let root_display_none = doc.nodes[root]
        .primary_styles()
        .map(|s| {
            let display = s.clone_display();
            display.outside() == DisplayOutside::None || display.inside() == DisplayInside::Contents
        })
        .unwrap_or(false);

    let root_ctm = match viewbox {
        // Zero-area viewBox -> nothing renders, but the fragment still
        // exists (empty `nodes`) rather than being entirely absent, so
        // `id_map` (used by other fragments referencing into this one, were
        // that ever legal it isn't, and by devtools) stays usable.
        Some(vb) if vb.width() > 0.0 && vb.height() > 0.0 => {
            viewport::viewbox_to_viewport_ctm(vb, viewport, par)
        }
        _ => viewport::identity_ctm(),
    };

    let mut nodes = Vec::new();
    if !root_display_none && !(viewbox.is_some_and(|vb| vb.width() <= 0.0 || vb.height() <= 0.0)) {
        let mut budget = MAX_INSTANCED_NODES;
        let mut font_ctx_guard = doc.font_ctx.lock().unwrap();
        // `LayoutContext` isn't stored per-document as an SVG-specific field;
        // a fresh one is cheap (it's a scratch buffer pool, not shape data)
        // and is dropped at the end of construction.
        let mut layout_ctx: LayoutContext<TextBrush> = LayoutContext::new();
        let scale = doc.viewport.scale();

        for &child in doc.nodes[root].children.iter() {
            walk(
                doc,
                child,
                root_ctm,
                None,
                &id_map,
                viewport,
                &mut nodes,
                0,
                &mut budget,
                &mut font_ctx_guard,
                &mut layout_ctx,
                scale,
            );
        }
    }

    SvgContext {
        root,
        viewport,
        viewbox,
        preserve_aspect_ratio: par,
        root_ctm,
        nodes,
        id_map,
    }
}

#[allow(clippy::too_many_arguments)]
fn walk(
    doc: &BaseDocument,
    node_id: NodeId,
    parent_ctm: Affine,
    parent_idx: Option<u32>,
    id_map: &HashMap<Atom, NodeId>,
    viewport: Size,
    nodes: &mut Vec<SvgNode>,
    use_depth: u32,
    budget: &mut usize,
    font_ctx: &mut FontContext,
    layout_ctx: &mut LayoutContext<TextBrush>,
    scale: f32,
) {
    if *budget == 0 {
        return;
    }

    let node: &crate::Node = &doc.nodes[node_id];
    let Some(elem) = node.data.downcast_element() else {
        return;
    };
    let tag = elem.name.local.as_ref();

    if is_non_rendered_container(tag) {
        return;
    }

    // display:none / visibility:hidden (hidden node not painted, but
    // its visible descendants still are, so `visibility` alone must not
    // prune the subtree, only `display:none` does).
    if let Some(style) = node.primary_styles() {
        if style.clone_display().outside() == DisplayOutside::None {
            return;
        }
    }

    *budget -= 1;

    let local_transform = raw_attr(elem.attrs(), "transform")
        .map(geometry::parse_transform_list)
        .unwrap_or(Affine::IDENTITY);
    let ctm = parent_ctm * local_transform;

    let diag = geometry::diagonal_basis(viewport.width, viewport.height);
    let attrs = elem.attrs();

    match tag {
        "g" | "a" => {
            let idx = nodes.len() as u32;
            nodes.push(SvgNode {
                dom_id: node_id,
                parent: parent_idx,
                ctm,
                kind: SvgNodeKind::Group,
                bbox: Rect::ZERO,
            });
            for &child in node.children.iter() {
                walk(
                    doc,
                    child,
                    ctm,
                    Some(idx),
                    id_map,
                    viewport,
                    nodes,
                    use_depth,
                    budget,
                    font_ctx,
                    layout_ctx,
                    scale,
                );
            }
        }

        // A nested `<svg>` establishes its own viewport: an `x`/`y`/`width`/`height` rect in
        // the parent's user space, with its own `viewBox`/ `preserveAspectRatio` scaling
        // descendants into it and the percentage basis switching to this inner viewport for them.
        "svg" => {
            let x = geometry::parse_coord_or_zero(raw_attr(attrs, "x"), viewport.width);
            let y = geometry::parse_coord_or_zero(raw_attr(attrs, "y"), viewport.height);
            let w = raw_attr(attrs, "width")
                .and_then(|v| geometry::parse_coord(v, viewport.width))
                .unwrap_or(viewport.width);
            let h = raw_attr(attrs, "height")
                .and_then(|v| geometry::parse_coord(v, viewport.height))
                .unwrap_or(viewport.height);

            let idx = nodes.len() as u32;
            let viewport_ctm = ctm * Affine::translate((x, y));
            nodes.push(SvgNode {
                dom_id: node_id,
                parent: parent_idx,
                ctm: viewport_ctm,
                kind: SvgNodeKind::Group,
                bbox: Rect::new(0.0, 0.0, w.max(0.0), h.max(0.0)),
            });

            // A zero-area inner viewport renders nothing.
            if w <= 0.0 || h <= 0.0 {
                return;
            }

            let inner_viewport = Size::new(w, h);
            let inner_vb = raw_attr(attrs, "viewBox").and_then(viewport::parse_viewbox);
            let inner_par = raw_attr(attrs, "preserveAspectRatio")
                .map(viewport::parse_preserve_aspect_ratio)
                .unwrap_or_default();
            let inner_ctm = match inner_vb {
                Some(vb) if vb.width() > 0.0 && vb.height() > 0.0 => {
                    viewport_ctm * viewport::viewbox_to_viewport_ctm(vb, inner_viewport, inner_par)
                }
                _ => viewport_ctm,
            };

            for &child in node.children.iter() {
                walk(
                    doc,
                    child,
                    inner_ctm,
                    Some(idx),
                    id_map,
                    inner_viewport,
                    nodes,
                    use_depth,
                    budget,
                    font_ctx,
                    layout_ctx,
                    scale,
                );
            }
        }

        "rect" => {
            let x = geometry::parse_coord_or_zero(raw_attr(attrs, "x"), viewport.width);
            let y = geometry::parse_coord_or_zero(raw_attr(attrs, "y"), viewport.height);
            let w = raw_attr(attrs, "width").and_then(|v| geometry::parse_coord(v, viewport.width));
            let h =
                raw_attr(attrs, "height").and_then(|v| geometry::parse_coord(v, viewport.height));
            let rx = raw_attr(attrs, "rx").and_then(|v| geometry::parse_coord(v, viewport.width));
            let ry = raw_attr(attrs, "ry").and_then(|v| geometry::parse_coord(v, viewport.height));
            let (rx, ry) = geometry::resolve_rect_radii(rx, ry, w.unwrap_or(0.0), h.unwrap_or(0.0));
            if let Some(path) =
                geometry::rect_path(x, y, w.unwrap_or(0.0), h.unwrap_or(0.0), rx, ry)
            {
                push_shape(nodes, node_id, parent_idx, ctm, path);
            }
        }

        "circle" => {
            let cx = geometry::parse_coord_or_zero(raw_attr(attrs, "cx"), viewport.width);
            let cy = geometry::parse_coord_or_zero(raw_attr(attrs, "cy"), viewport.height);
            let r = geometry::parse_coord_or_zero(raw_attr(attrs, "r"), diag);
            if let Some(path) = geometry::circle_path(cx, cy, r) {
                push_shape(nodes, node_id, parent_idx, ctm, path);
            }
        }

        "ellipse" => {
            let cx = geometry::parse_coord_or_zero(raw_attr(attrs, "cx"), viewport.width);
            let cy = geometry::parse_coord_or_zero(raw_attr(attrs, "cy"), viewport.height);
            let rx = raw_attr(attrs, "rx").and_then(|v| geometry::parse_coord(v, viewport.width));
            let ry = raw_attr(attrs, "ry").and_then(|v| geometry::parse_coord(v, viewport.height));
            // One auto radius takes the other's (already-resolved) value.
            let (rx, ry) = match (rx, ry) {
                (Some(rx), Some(ry)) => (rx, ry),
                (Some(rx), None) => (rx, rx),
                (None, Some(ry)) => (ry, ry),
                (None, None) => (0.0, 0.0),
            };
            if let Some(path) = geometry::ellipse_path(cx, cy, rx, ry) {
                push_shape(nodes, node_id, parent_idx, ctm, path);
            }
        }

        "line" => {
            let x1 = geometry::parse_coord_or_zero(raw_attr(attrs, "x1"), viewport.width);
            let y1 = geometry::parse_coord_or_zero(raw_attr(attrs, "y1"), viewport.height);
            let x2 = geometry::parse_coord_or_zero(raw_attr(attrs, "x2"), viewport.width);
            let y2 = geometry::parse_coord_or_zero(raw_attr(attrs, "y2"), viewport.height);
            let path = geometry::line_path(x1, y1, x2, y2);
            push_shape(nodes, node_id, parent_idx, ctm, path);
        }

        "polyline" => {
            let pts = geometry::parse_points(raw_attr(attrs, "points").unwrap_or(""));
            if let Some(path) = geometry::polyline_path(&pts) {
                push_shape(nodes, node_id, parent_idx, ctm, path);
            }
        }

        "polygon" => {
            let pts = geometry::parse_points(raw_attr(attrs, "points").unwrap_or(""));
            if let Some(path) = geometry::polygon_path(&pts) {
                push_shape(nodes, node_id, parent_idx, ctm, path);
            }
        }

        "path" => {
            if let Some(path) = raw_attr(attrs, "d").and_then(geometry::path_from_d) {
                push_shape(nodes, node_id, parent_idx, ctm, path);
            }
        }

        "text" => {
            if let Some(run) = text::shape_text(doc, node_id, font_ctx, layout_ctx, scale) {
                let x = geometry::parse_coord_or_zero(raw_attr(attrs, "x"), viewport.width);
                let y = geometry::parse_coord_or_zero(raw_attr(attrs, "y"), viewport.height);
                let text_ctm = ctm * Affine::translate((x, y));
                let full_width = run.layout.full_width() as f64;
                let bbox = Rect::new(0.0, 0.0, full_width, run.layout.height() as f64);
                nodes.push(SvgNode {
                    dom_id: node_id,
                    parent: parent_idx,
                    ctm: text_ctm,
                    kind: SvgNodeKind::Text(Box::new(run)),
                    bbox,
                });
            }
        }

        "foreignObject" => {
            let x = geometry::parse_coord_or_zero(raw_attr(attrs, "x"), viewport.width);
            let y = geometry::parse_coord_or_zero(raw_attr(attrs, "y"), viewport.height);
            let w = geometry::parse_coord_or_zero(raw_attr(attrs, "width"), viewport.width);
            let h = geometry::parse_coord_or_zero(raw_attr(attrs, "height"), viewport.height);
            if w > 0.0 && h > 0.0 {
                let fo_ctm = ctm * Affine::translate((x, y));
                nodes.push(SvgNode {
                    dom_id: node_id,
                    parent: parent_idx,
                    ctm: fo_ctm,
                    kind: SvgNodeKind::ForeignObject,
                    bbox: Rect::new(0.0, 0.0, w, h),
                });
            }
        }

        "image" => {
            nodes.push(SvgNode {
                dom_id: node_id,
                parent: parent_idx,
                ctm,
                kind: SvgNodeKind::Image,
                bbox: Rect::ZERO,
            });
        }

        "use" => {
            // Depth cap + ancestor-self cycle guard.
            if use_depth >= resolve::MAX_REF_DEPTH {
                return;
            }
            let Some(target) = resolve::resolve_href(id_map, attrs) else {
                return;
            };
            if target == node_id || resolve::is_ancestor_or_self(doc, target, node_id) {
                return;
            }
            let target_tag = doc.nodes[target]
                .data
                .downcast_element()
                .map(|e| e.name.local.clone());
            // A `<use>` targeting `<symbol>`/`<svg>` establishes a viewport of `use.width x use.height`.
            let establishes_viewport =
                matches!(target_tag.as_deref(), Some("symbol") | Some("svg"));
            let x = geometry::parse_coord_or_zero(raw_attr(attrs, "x"), viewport.width);
            let y = geometry::parse_coord_or_zero(raw_attr(attrs, "y"), viewport.height);
            let use_ctm = ctm * Affine::translate((x, y));

            let idx = nodes.len() as u32;
            nodes.push(SvgNode {
                dom_id: node_id,
                parent: parent_idx,
                ctm: use_ctm,
                kind: SvgNodeKind::Use { target },
                bbox: Rect::ZERO,
            });

            if establishes_viewport {
                let use_w = raw_attr(attrs, "width")
                    .and_then(|v| geometry::parse_coord(v, viewport.width))
                    .unwrap_or(viewport.width);
                let use_h = raw_attr(attrs, "height")
                    .and_then(|v| geometry::parse_coord(v, viewport.height))
                    .unwrap_or(viewport.height);
                let target_attrs = doc.nodes[target].attrs().unwrap_or(&[]);
                let inner_vb = raw_attr(target_attrs, "viewBox").and_then(viewport::parse_viewbox);
                let inner_par = raw_attr(target_attrs, "preserveAspectRatio")
                    .map(viewport::parse_preserve_aspect_ratio)
                    .unwrap_or_default();
                let inner_ctm = match inner_vb {
                    Some(vb) if vb.width() > 0.0 && vb.height() > 0.0 => {
                        use_ctm
                            * viewport::viewbox_to_viewport_ctm(
                                vb,
                                Size::new(use_w, use_h),
                                inner_par,
                            )
                    }
                    _ => use_ctm,
                };
                for &child in doc.nodes[target].children.iter() {
                    walk(
                        doc,
                        child,
                        inner_ctm,
                        Some(idx),
                        id_map,
                        viewport,
                        nodes,
                        use_depth + 1,
                        budget,
                        font_ctx,
                        layout_ctx,
                        scale,
                    );
                }
            } else {
                walk(
                    doc,
                    target,
                    use_ctm,
                    Some(idx),
                    id_map,
                    viewport,
                    nodes,
                    use_depth + 1,
                    budget,
                    font_ctx,
                    layout_ctx,
                    scale,
                );
            }
        }

        // Unrecognized element (including `<switch>`, `<view>`, custom elements): degrade gracefully by treating
        // it as a transparent group rather than dropping its subtree.
        _ => {
            for &child in node.children.iter() {
                walk(
                    doc, child, ctm, parent_idx, id_map, viewport, nodes, use_depth, budget,
                    font_ctx, layout_ctx, scale,
                );
            }
        }
    }
}

fn push_shape(
    nodes: &mut Vec<SvgNode>,
    dom_id: NodeId,
    parent: Option<u32>,
    ctm: Affine,
    path: kurbo::BezPath,
) {
    let bbox = path.bounding_box();
    nodes.push(SvgNode {
        dom_id,
        parent,
        ctm,
        kind: SvgNodeKind::Shape(path),
        bbox,
    });
}
