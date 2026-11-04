//! Element scrolling and argument conversion shared with Window.

use crate::dom::to_rust_string;
use crate::dom::{dom_ctx, this_node_id};
use blitz_dom::{NodeId, ScrollBehavior, ScrollLogicalPosition};
use boa_engine::{Context, JsResult, JsValue};
use boa_engine::{JsNativeError, js_string};

// === Scrolling ===

/// The current scroll offset of an element. The root element scrolls the
/// viewport (per the CSS overflow propagation rules), so its offset is the
/// viewport scroll offset.
fn current_scroll_offset(doc: &blitz_dom::BaseDocument, node_id: NodeId) -> blitz_dom::Point<f64> {
    if doc
        .try_root_element()
        .is_some_and(|root| root.id == node_id)
    {
        doc.viewport_scroll()
    } else {
        doc.get_node(node_id)
            .filter(|node| node.element_data().is_some())
            .map(|node| *node.scroll_offset())
            .unwrap_or(blitz_dom::Point { x: 0.0, y: 0.0 })
    }
}

/// Convert a JS value to a scroll coordinate: WebIDL `unrestricted double`,
/// with non-finite values normalized to 0 per the cssom-view spec.
fn scroll_coord(value: &JsValue, context: &mut Context) -> JsResult<f64> {
    let number = value.to_number(context)?;
    Ok(if number.is_finite() { number } else { 0.0 })
}

/// Parse a WebIDL `ScrollBehavior` enumeration value (`undefined` maps to the
/// default, "auto"; other invalid values throw a `TypeError`)
fn parse_scroll_behavior(value: &JsValue, context: &mut Context) -> JsResult<ScrollBehavior> {
    if value.is_undefined() {
        return Ok(ScrollBehavior::Auto);
    }
    let string = to_rust_string(value, context)?;
    match string.as_str() {
        "auto" => Ok(ScrollBehavior::Auto),
        "instant" => Ok(ScrollBehavior::Instant),
        "smooth" => Ok(ScrollBehavior::Smooth),
        _ => Err(JsNativeError::typ()
            .with_message(format!(
                "{string:?} is not a valid value for enumeration ScrollBehavior"
            ))
            .into()),
    }
}

/// Parsed arguments of the `scrollTo(x, y)` / `scrollTo(options)` overloads
/// shared by the `Element` and `Window` scroll methods. `None` coordinates
/// were not specified (and default to the current position for `scrollTo`,
/// or a zero delta for `scrollBy`).
pub(crate) struct ScrollToArgs {
    pub(crate) left: Option<f64>,
    pub(crate) top: Option<f64>,
    pub(crate) behavior: ScrollBehavior,
}

/// Parse the WebIDL `scrollTo(x, y)` / `scrollTo(options)` overloads: two or
/// more arguments are coordinates; a single argument must be a `ScrollToOptions`
/// dictionary (a single non-object argument throws a `TypeError`, per WebIDL
/// dictionary conversion).
pub(crate) fn parse_scroll_to_args(
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<ScrollToArgs> {
    if args.len() >= 2 {
        return Ok(ScrollToArgs {
            left: Some(scroll_coord(&args[0], context)?),
            top: Some(scroll_coord(&args[1], context)?),
            behavior: ScrollBehavior::Auto,
        });
    }
    match args.first() {
        None => Ok(ScrollToArgs {
            left: None,
            top: None,
            behavior: ScrollBehavior::Auto,
        }),
        Some(value) if value.is_null_or_undefined() => Ok(ScrollToArgs {
            left: None,
            top: None,
            behavior: ScrollBehavior::Auto,
        }),
        Some(value) => {
            let Some(options) = value.as_object() else {
                return Err(JsNativeError::typ()
                    .with_message("value cannot be converted to a ScrollToOptions dictionary")
                    .into());
            };
            let left = options.get(js_string!("left"), context)?;
            let left = (!left.is_undefined())
                .then(|| scroll_coord(&left, context))
                .transpose()?;
            let top = options.get(js_string!("top"), context)?;
            let top = (!top.is_undefined())
                .then(|| scroll_coord(&top, context))
                .transpose()?;
            let behavior = options.get(js_string!("behavior"), context)?;
            let behavior = parse_scroll_behavior(&behavior, context)?;
            Ok(ScrollToArgs {
                left,
                top,
                behavior,
            })
        }
    }
}

pub(super) fn get_scroll_top(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let doc = ctx.doc.borrow();
    Ok(JsValue::from(current_scroll_offset(&doc, node_id).y))
}

pub(super) fn get_scroll_left(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let doc = ctx.doc.borrow();
    Ok(JsValue::from(current_scroll_offset(&doc, node_id).x))
}

pub(super) fn set_scroll_top(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let value = scroll_coord(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.resolve(0.0);
    let current = current_scroll_offset(&doc, node_id);
    doc.scroll_to(node_id, current.x, value, ScrollBehavior::Auto);
    Ok(JsValue::undefined())
}

pub(super) fn set_scroll_left(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let value = scroll_coord(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.resolve(0.0);
    let current = current_scroll_offset(&doc, node_id);
    doc.scroll_to(node_id, value, current.y, ScrollBehavior::Auto);
    Ok(JsValue::undefined())
}

pub(super) fn scroll_to_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let parsed = parse_scroll_to_args(args, context)?;
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.resolve(0.0);
    let current = current_scroll_offset(&doc, node_id);
    doc.scroll_to(
        node_id,
        parsed.left.unwrap_or(current.x),
        parsed.top.unwrap_or(current.y),
        parsed.behavior,
    );
    Ok(JsValue::undefined())
}

pub(super) fn scroll_by_method(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let parsed = parse_scroll_to_args(args, context)?;
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.resolve(0.0);
    doc.scroll_by(
        node_id,
        parsed.left.unwrap_or(0.0),
        parsed.top.unwrap_or(0.0),
        parsed.behavior,
    );
    Ok(JsValue::undefined())
}

/// Parse a WebIDL `ScrollLogicalPosition` enumeration value (`undefined` maps
/// to the given default; other invalid values throw a `TypeError`)
fn parse_scroll_logical_position(
    value: &JsValue,
    default: ScrollLogicalPosition,
    context: &mut Context,
) -> JsResult<ScrollLogicalPosition> {
    if value.is_undefined() {
        return Ok(default);
    }
    let string = to_rust_string(value, context)?;
    match string.as_str() {
        "start" => Ok(ScrollLogicalPosition::Start),
        "center" => Ok(ScrollLogicalPosition::Center),
        "end" => Ok(ScrollLogicalPosition::End),
        "nearest" => Ok(ScrollLogicalPosition::Nearest),
        _ => Err(JsNativeError::typ()
            .with_message(format!(
                "{string:?} is not a valid value for enumeration ScrollLogicalPosition"
            ))
            .into()),
    }
}

pub(super) fn scroll_into_view(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let arg = args.first().cloned().unwrap_or_default();
    // WebIDL `(boolean or ScrollIntoViewOptions)`: objects (and null/undefined)
    // convert to the options dictionary, everything else converts to a boolean
    // (`true` aligns to the start, `false` to the end).
    let (behavior, block, inline) = if let Some(options) = arg.as_object() {
        let behavior = options.get(js_string!("behavior"), context)?;
        let behavior = parse_scroll_behavior(&behavior, context)?;
        let block = options.get(js_string!("block"), context)?;
        let block = parse_scroll_logical_position(&block, ScrollLogicalPosition::Start, context)?;
        let inline = options.get(js_string!("inline"), context)?;
        let inline =
            parse_scroll_logical_position(&inline, ScrollLogicalPosition::Nearest, context)?;
        (behavior, block, inline)
    } else if arg.is_null_or_undefined() || arg.to_boolean() {
        (
            ScrollBehavior::Auto,
            ScrollLogicalPosition::Start,
            ScrollLogicalPosition::Nearest,
        )
    } else {
        (
            ScrollBehavior::Auto,
            ScrollLogicalPosition::End,
            ScrollLogicalPosition::Nearest,
        )
    };

    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.resolve(0.0);
    doc.scroll_into_view(node_id, behavior, block, inline);
    Ok(JsValue::undefined())
}
