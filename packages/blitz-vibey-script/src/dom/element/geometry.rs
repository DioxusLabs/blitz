//! Layout-dependent element dimensions and bounding rectangles.

use crate::dom::node_or_null;
use crate::dom::{dom_ctx, this_node_id};
use blitz_dom::NodeId;
use boa_engine::js_string;
use boa_engine::object::{JsObject, ObjectInitializer};
use boa_engine::property::Attribute as PropAttribute;
use boa_engine::{Context, JsResult, JsValue};

// === Geometry ===

/// Look up a node and compute a geometry value from its layout, resolving
/// style/layout first so that the values reflect any recent DOM mutations.
/// Returns 0.0 if the node does not exist.
fn layout_value(
    this: &JsValue,
    context: &mut Context,
    f: impl FnOnce(&blitz_dom::Node) -> f32,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.resolve(0.0);
    let value = doc.get_node(node_id).map(f).unwrap_or(0.0);
    Ok(JsValue::from(value as f64))
}

/// `offsetLeft`/`offsetTop`/`offsetWidth`/`offsetHeight`: the node's border box
/// relative to the padding edge of its `offsetParent` (blitz-dom's `offset_rect`
/// implements the offsetParent resolution and the fragment bounding box of
/// non-atomic inline elements)
fn offset_value(
    this: &JsValue,
    context: &mut Context,
    f: impl FnOnce(&blitz_dom::BoundingRect) -> f64,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.resolve(0.0);
    let value = doc.offset_rect(node_id).map(|rect| f(&rect)).unwrap_or(0.0);
    Ok(JsValue::from(value.round()))
}

pub(super) fn offset_width(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    offset_value(this, context, |rect| rect.width)
}

pub(super) fn offset_height(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    offset_value(this, context, |rect| rect.height)
}

pub(super) fn offset_left(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    offset_value(this, context, |rect| rect.x)
}

pub(super) fn offset_top(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    offset_value(this, context, |rect| rect.y)
}

/// `clientWidth`/`clientHeight`. For the root element (in no-quirks mode, which
/// is all Blitz supports) these are the viewport dimensions minus scrollbars.
fn client_size(
    this: &JsValue,
    context: &mut Context,
    axis: impl FnOnce(&blitz_dom::Node) -> f32,
    viewport_axis: impl FnOnce(&blitz_dom::Node, (f32, f32)) -> f32,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.resolve(0.0);
    let Some(node) = doc.get_node(node_id) else {
        return Ok(JsValue::from(0.0));
    };
    let value = if node_id == doc.root_element().id {
        let viewport = doc.viewport();
        let scale = viewport.scale();
        let size = (
            viewport.window_size.0 as f32 / scale,
            viewport.window_size.1 as f32 / scale,
        );
        viewport_axis(node, size)
    } else {
        axis(node)
    };
    Ok(JsValue::from(value.round() as f64))
}

pub(super) fn client_width(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    client_size(
        this,
        context,
        |node| node.client_width(),
        |node, (w, _)| w - node.final_layout().scrollbar_size.width,
    )
}

/// `offsetParent`: the nearest positioned ancestor (or `body`/table cell), as
/// resolved by blitz-dom; `null` when there is none.
pub(super) fn offset_parent(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let parent_id = {
        let mut doc = ctx.doc.borrow_mut();
        doc.resolve(0.0);
        doc.get_node(node_id)
            .and_then(|node| node.offset_parent())
            .map(|parent| parent.id)
    };
    Ok(node_or_null(&ctx, parent_id, context))
}

// `clientTop`/`clientLeft`: the border widths (jQuery's `.offset()` subtracts
// the root element's from `getBoundingClientRect`).

pub(super) fn client_top(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    layout_value(this, context, |node| node.final_layout().border.top.round())
}

pub(super) fn client_left(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    layout_value(this, context, |node| {
        node.final_layout().border.left.round()
    })
}

pub(super) fn client_height(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    client_size(
        this,
        context,
        |node| node.client_height(),
        |node, (_, h)| h - node.final_layout().scrollbar_size.height,
    )
}

pub(super) fn scroll_width(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    layout_value(this, context, |node| node.scroll_width().round())
}

pub(super) fn scroll_height(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    layout_value(this, context, |node| node.scroll_height().round())
}

/// Build a `DOMRect`-shaped object
fn make_rect_object(context: &mut Context, x: f64, y: f64, width: f64, height: f64) -> JsObject {
    ObjectInitializer::new(context)
        .property(js_string!("x"), x, PropAttribute::all())
        .property(js_string!("y"), y, PropAttribute::all())
        .property(js_string!("width"), width, PropAttribute::all())
        .property(js_string!("height"), height, PropAttribute::all())
        .property(js_string!("left"), x, PropAttribute::all())
        .property(js_string!("top"), y, PropAttribute::all())
        .property(js_string!("right"), x + width, PropAttribute::all())
        .property(js_string!("bottom"), y + height, PropAttribute::all())
        .build()
}

/// Does the node generate any boxes? (`getClientRects()` returns an empty list
/// for boxless nodes, e.g. `display: none` or detached elements)
fn node_has_boxes(doc: &blitz_dom::BaseDocument, node_id: NodeId) -> bool {
    doc.get_node(node_id).is_some_and(|node| node.has_boxes())
}

pub(super) fn get_bounding_client_rect(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    ctx.doc.borrow_mut().resolve(0.0);
    let rect = ctx.doc.borrow().get_client_bounding_rect(node_id);
    let (x, y, width, height) = match rect {
        Some(rect) => (rect.x, rect.y, rect.width, rect.height),
        None => (0.0, 0.0, 0.0, 0.0),
    };
    Ok(make_rect_object(context, x, y, width, height).into())
}

pub(super) fn get_client_rects(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    ctx.doc.borrow_mut().resolve(0.0);

    // One rect per box fragment: a single border-box rect for nodes with their
    // own layout box, one rect per line box for non-atomic inline elements,
    // and an empty list for boxless (e.g. `display: none`) elements
    let rects = {
        let doc = ctx.doc.borrow();
        if node_has_boxes(&doc, node_id) {
            doc.node_client_rects(node_id)
        } else {
            Vec::new()
        }
    };

    let rect_objects: Vec<JsValue> = rects
        .into_iter()
        .map(|rect| make_rect_object(context, rect.x, rect.y, rect.width, rect.height).into())
        .collect();
    Ok(boa_engine::object::builtins::JsArray::from_iter(rect_objects, context).into())
}
