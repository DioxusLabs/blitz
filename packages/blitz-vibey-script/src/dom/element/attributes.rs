//! Element attributes and reflected HTML attributes.

use crate::dom::{define_value, js_str, to_rust_string};
use crate::dom::{dom_ctx, this_node_id};
use crate::state::DomCtx;
use blitz_dom::{LocalName, NodeId, QualName};
use boa_engine::object::JsObject;
use boa_engine::{Context, JsResult, JsValue};

/// Construct a `QualName` for an attribute (no namespace)
pub(crate) fn attr_name(local: &str) -> QualName {
    QualName::new(None, markup5ever::ns!(), LocalName::from(local))
}

// === Attribute helpers ===

pub(crate) fn read_attr(ctx: &DomCtx, node_id: NodeId, name: &str) -> Option<String> {
    let doc = ctx.doc.borrow();
    let node = doc.get_node(node_id)?;
    let element = node.element_data()?;
    element
        .attrs()
        .iter()
        .find(|attr| &*attr.name.local == name)
        .map(|attr| attr.value.clone())
}

pub(crate) fn write_attr(ctx: &DomCtx, node_id: NodeId, name: &str, value: &str) {
    let mut doc = ctx.doc.borrow_mut();
    doc.mutate().set_attribute(node_id, attr_name(name), value);
}

pub(super) fn clear_attr(ctx: &DomCtx, node_id: NodeId, name: &str) {
    let mut doc = ctx.doc.borrow_mut();
    doc.mutate().clear_attribute(node_id, attr_name(name));
}

pub(super) fn attr_getter(name: &str, this: &JsValue, context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    Ok(js_str(&read_attr(&ctx, node_id, name).unwrap_or_default()))
}

pub(super) fn attr_setter(
    name: &str,
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let value = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    write_attr(&ctx, node_id, name, &value);
    Ok(JsValue::undefined())
}

// === Attributes ===

pub(super) fn get_attribute(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let name = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?
        .to_ascii_lowercase();
    match read_attr(&ctx, node_id, &name) {
        Some(value) => Ok(js_str(&value)),
        None => Ok(JsValue::null()),
    }
}

/// All attributes of the element as `(name, value)` pairs, in order.
fn read_attrs(ctx: &DomCtx, node_id: NodeId) -> Vec<(String, String)> {
    let doc = ctx.doc.borrow();
    doc.get_node(node_id)
        .and_then(|node| node.element_data())
        .map(|element| {
            element
                .attrs()
                .iter()
                .map(|attr| (attr.name.local.to_string(), attr.value.clone()))
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn get_attribute_names(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let names: Vec<JsValue> = read_attrs(&ctx, node_id)
        .into_iter()
        .map(|(name, _)| js_str(&name))
        .collect();
    Ok(boa_engine::object::builtins::JsArray::from_iter(names, context).into())
}

/// `element.attributes`: a snapshot `NamedNodeMap` — an array-like of
/// `Attr`-shaped objects (`name`, `value`, `nodeName`, `nodeValue`), which is
/// what jQuery's `.data()` walks to collect `data-*` attributes.
pub(super) fn attributes(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let items: Vec<JsValue> = read_attrs(&ctx, node_id)
        .into_iter()
        .map(|(name, value)| {
            let attr = JsObject::with_object_proto(context.intrinsics());
            define_value(&attr, "name", js_str(&name), context);
            define_value(&attr, "nodeName", js_str(&name), context);
            define_value(&attr, "localName", js_str(&name), context);
            define_value(&attr, "value", js_str(&value), context);
            define_value(&attr, "nodeValue", js_str(&value), context);
            define_value(&attr, "specified", JsValue::from(true), context);
            attr.into()
        })
        .collect();
    let map = boa_engine::object::builtins::JsArray::from_iter(items.iter().cloned(), context);
    Ok(map.into())
}

pub(super) fn set_attribute(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let name = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?
        .to_ascii_lowercase();
    let value = to_rust_string(args.get(1).unwrap_or(&JsValue::undefined()), context)?;
    write_attr(&ctx, node_id, &name, &value);
    Ok(JsValue::undefined())
}

pub(super) fn remove_attribute(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let name = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?
        .to_ascii_lowercase();
    clear_attr(&ctx, node_id, &name);
    Ok(JsValue::undefined())
}

pub(super) fn has_attribute(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let name = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?
        .to_ascii_lowercase();
    Ok(JsValue::from(read_attr(&ctx, node_id, &name).is_some()))
}

// === Reflected DOM properties ===

pub(super) fn get_id(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let _ = args;
    attr_getter("id", this, context)
}
pub(super) fn set_id(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    attr_setter("id", this, args, context)
}

pub(super) fn get_class_name(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let _ = args;
    attr_getter("class", this, context)
}
pub(super) fn set_class_name(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    attr_setter("class", this, args, context)
}

/// `HTMLStyleElement.disabled` / `HTMLLinkElement.disabled` control the
/// disabled flag of the element's associated stylesheet rather than (only) a
/// content attribute.
fn is_stylesheet_owner_tag(tag: &LocalName) -> bool {
    matches!(&**tag, "style" | "link")
}

pub(super) fn get_disabled(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    if element_local_name(&ctx, node_id).is_some_and(|tag| is_stylesheet_owner_tag(&tag)) {
        return Ok(JsValue::from(ctx.doc.borrow().stylesheet_disabled(node_id)));
    }
    Ok(JsValue::from(
        read_attr(&ctx, node_id, "disabled").is_some(),
    ))
}

pub(super) fn set_disabled(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let disabled = args.first().map(JsValue::to_boolean).unwrap_or(false);
    if let Some(tag) = element_local_name(&ctx, node_id) {
        if is_stylesheet_owner_tag(&tag) {
            ctx.doc
                .borrow_mut()
                .set_stylesheet_disabled(node_id, disabled);
            if &*tag == "style" {
                return Ok(JsValue::undefined());
            }
        }
    }
    if disabled {
        write_attr(&ctx, node_id, "disabled", "");
    } else {
        clear_attr(&ctx, node_id, "disabled");
    }
    Ok(JsValue::undefined())
}

pub(super) fn get_hidden(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    Ok(JsValue::from(read_attr(&ctx, node_id, "hidden").is_some()))
}

pub(super) fn set_hidden(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let hidden = args.first().map(JsValue::to_boolean).unwrap_or(false);
    if hidden {
        write_attr(&ctx, node_id, "hidden", "");
    } else {
        clear_attr(&ctx, node_id, "hidden");
    }
    Ok(JsValue::undefined())
}

// === `width`/`height` (`unsigned long` reflected content attributes) ===
//
// Only the elements whose `width`/`height` content attributes are reflected as
// numbers by their IDL interface (`HTMLCanvasElement`, `HTMLImageElement`, ...)
// get the reflected property. A canvas defaults to its 300x150 bitmap size.

fn element_local_name(ctx: &DomCtx, node_id: NodeId) -> Option<LocalName> {
    let doc = ctx.doc.borrow();
    let element = doc.get_node(node_id)?.element_data()?;
    Some(element.name.local.clone())
}

fn dimension_attr_default(tag: &LocalName, attr: &str) -> Option<u32> {
    match &**tag {
        "canvas" => Some(if attr == "width" { 300 } else { 150 }),
        "img" | "video" | "iframe" | "embed" | "object" => Some(0),
        _ => None,
    }
}

/// Parse a reflected `unsigned long` content attribute (HTML "rules for parsing
/// non-negative integers"), falling back to `default` on a missing/invalid value
fn parse_unsigned_long_attr(value: Option<String>, default: u32) -> u32 {
    value
        .as_deref()
        .map(str::trim)
        .and_then(|value| {
            let value = value.strip_prefix('+').unwrap_or(value);
            let digits: &str = value
                .split(|c: char| !c.is_ascii_digit())
                .next()
                .unwrap_or("");
            digits.parse::<u32>().ok()
        })
        .filter(|value| *value <= 2147483647)
        .unwrap_or(default)
}

fn dimension_getter(attr: &str, this: &JsValue, context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let Some(tag) = element_local_name(&ctx, node_id) else {
        return Ok(JsValue::undefined());
    };
    let Some(default) = dimension_attr_default(&tag, attr) else {
        return Ok(JsValue::undefined());
    };
    let value = parse_unsigned_long_attr(read_attr(&ctx, node_id, attr), default);
    Ok(JsValue::from(value))
}

fn dimension_setter(
    attr: &str,
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let Some(tag) = element_local_name(&ctx, node_id) else {
        return Ok(JsValue::undefined());
    };
    let Some(default) = dimension_attr_default(&tag, attr) else {
        // Not a reflected attribute on this element: behave like a plain
        // expando property so `div.width = x` still round-trips.
        if let Some(obj) = this.as_object() {
            obj.create_data_property_or_throw(
                boa_engine::JsString::from(attr),
                args.first().cloned().unwrap_or_default(),
                context,
            )?;
        }
        return Ok(JsValue::undefined());
    };
    // WebIDL `unsigned long` conversion: ToNumber, then modulo 2^32; values
    // outside the (0, 2^31) range fall back to the default
    let number = args
        .first()
        .unwrap_or(&JsValue::undefined())
        .to_number(context)?;
    let value = if number.is_finite() {
        let value = number.trunc().rem_euclid(4294967296.0) as u32;
        if value > 2147483647 { default } else { value }
    } else {
        0
    };
    write_attr(&ctx, node_id, attr, &value.to_string());
    Ok(JsValue::undefined())
}

pub(super) fn get_width(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    dimension_getter("width", this, context)
}
pub(super) fn set_width(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    dimension_setter("width", this, args, context)
}
pub(super) fn get_height(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    dimension_getter("height", this, context)
}
pub(super) fn set_height(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    dimension_setter("height", this, args, context)
}
