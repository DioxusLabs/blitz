//! A minimal `CSSStyleDeclaration` binding (`element.style`).

use blitz_dom::NodeId;
use boa_engine::object::JsObject;
use boa_engine::value::JsValue;
use boa_engine::{Context, JsResult};

use super::element::attr_name;
use super::{define_accessor, define_method, dom_ctx, js_str, this_node_id, to_rust_string};

pub(crate) fn init_style_proto(proto: &JsObject, context: &mut Context) {
    define_accessor(
        proto,
        "cssText",
        Some(get_css_text),
        Some(set_css_text),
        context,
    );
    define_method(proto, "setProperty", 2, set_property, context);
    define_method(proto, "removeProperty", 1, remove_property, context);
    define_method(proto, "getPropertyValue", 1, get_property_value, context);
}

/// The prototype for the read-only `CSSStyleDeclaration`s returned by
/// `getComputedStyle()`. Property reads return *resolved* values.
pub(crate) fn init_computed_style_proto(proto: &JsObject, context: &mut Context) {
    define_accessor(proto, "cssText", Some(get_empty_string), None, context);
    define_method(proto, "setProperty", 2, noop, context);
    define_method(proto, "removeProperty", 1, noop, context);
    define_method(
        proto,
        "getPropertyValue",
        1,
        get_resolved_property_value,
        context,
    );
}

fn get_empty_string(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(js_str(""))
}

fn noop(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::undefined())
}

fn get_resolved_property_value(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let name = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?
        .trim()
        .to_ascii_lowercase();

    let mut doc = ctx.doc.borrow_mut();
    // Resolved values of layout-dependent properties are used values, so make
    // sure style and layout are up to date before reading.
    doc.resolve(0.0);
    let value = doc.resolved_style_value(node_id, &name);
    Ok(js_str(&value))
}

/// Read the node's `style` attribute (empty string if unset)
fn read_style_attr(ctx: &crate::state::DomCtx, node_id: NodeId) -> String {
    ctx.doc
        .borrow()
        .get_node(node_id)
        .and_then(|node| node.attr(blitz_dom::local_name!("style")))
        .unwrap_or_default()
        .to_string()
}

fn write_style_attr(ctx: &crate::state::DomCtx, node_id: NodeId, style_attr: &str) {
    let mut doc = ctx.doc.borrow_mut();
    doc.mutate()
        .set_attribute(node_id, attr_name("style"), style_attr);
}

fn get_css_text(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let style_attr = read_style_attr(&ctx, node_id);
    let css = ctx.doc.borrow().style_attr_serialize(&style_attr);
    Ok(js_str(&css))
}

fn set_css_text(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let css = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    // Parse and re-serialize so that the stored attribute is canonical and
    // invalid declarations are dropped
    let css = ctx.doc.borrow().style_attr_serialize(&css);
    write_style_attr(&ctx, node_id, &css);
    Ok(JsValue::undefined())
}

fn set_property(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let name = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let value = to_rust_string(args.get(1).unwrap_or(&JsValue::undefined()), context)?;
    let priority = to_rust_string(args.get(2).unwrap_or(&JsValue::undefined()), context)?;
    let important = priority.eq_ignore_ascii_case("important");

    let style_attr = read_style_attr(&ctx, node_id);
    let new_style_attr =
        ctx.doc
            .borrow()
            .style_attr_set_property(&style_attr, &name, &value, important);
    // `None` means the declaration was invalid: ignore it (per CSSOM)
    if let Some(new_style_attr) = new_style_attr {
        write_style_attr(&ctx, node_id, &new_style_attr);
    }
    Ok(JsValue::undefined())
}

fn remove_property(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let name = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;

    let style_attr = read_style_attr(&ctx, node_id);
    let removed = ctx
        .doc
        .borrow()
        .style_attr_remove_property(&style_attr, &name);
    let Some((new_style_attr, removed_value)) = removed else {
        return Ok(js_str(""));
    };
    write_style_attr(&ctx, node_id, &new_style_attr);
    Ok(js_str(&removed_value))
}

fn get_property_value(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let name = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;

    let style_attr = read_style_attr(&ctx, node_id);
    let value = ctx.doc.borrow().style_attr_get_property(&style_attr, &name);
    Ok(js_str(&value))
}
