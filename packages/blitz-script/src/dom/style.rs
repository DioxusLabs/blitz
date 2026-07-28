//! A minimal `CSSStyleDeclaration` binding (`element.style`).

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

fn get_css_text(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let doc = ctx.doc.borrow();
    let css = doc
        .get_node(node_id)
        .and_then(|node| node.attr(blitz_dom::local_name!("style")))
        .unwrap_or_default();
    Ok(js_str(css))
}

fn set_css_text(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let css = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.mutate()
        .set_attribute(node_id, attr_name("style"), &css);
    Ok(JsValue::undefined())
}

/// Parse a style attribute string into (property, value) pairs.
///
/// This is a simplification: it does not handle `;` or `:` characters inside
/// values (e.g. in `url(...)` or quoted strings).
fn parse_declarations(style_attr: &str) -> Vec<(String, String)> {
    style_attr
        .split(';')
        .filter_map(|decl| decl.split_once(':'))
        .map(|(prop, value)| (prop.trim().to_string(), value.trim().to_string()))
        .filter(|(prop, _)| !prop.is_empty())
        .collect()
}

fn serialize_declarations(decls: &[(String, String)]) -> String {
    decls
        .iter()
        .map(|(prop, value)| format!("{prop}: {value};"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn update_style_attr(
    ctx: &crate::state::DomCtx,
    node_id: usize,
    f: impl FnOnce(&mut Vec<(String, String)>),
) {
    let mut doc = ctx.doc.borrow_mut();
    let style_attr = doc
        .get_node(node_id)
        .and_then(|node| node.attr(blitz_dom::local_name!("style")))
        .unwrap_or_default()
        .to_string();
    let mut decls = parse_declarations(&style_attr);
    f(&mut decls);
    let new_style = serialize_declarations(&decls);
    doc.mutate()
        .set_attribute(node_id, attr_name("style"), &new_style);
}

fn set_property(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let name = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let value = to_rust_string(args.get(1).unwrap_or(&JsValue::undefined()), context)?;
    update_style_attr(&ctx, node_id, |decls| {
        decls.retain(|(prop, _)| !prop.eq_ignore_ascii_case(&name));
        if !value.is_empty() {
            decls.push((name.to_ascii_lowercase(), value));
        }
    });
    Ok(JsValue::undefined())
}

fn remove_property(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let name = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let mut removed = String::new();
    update_style_attr(&ctx, node_id, |decls| {
        if let Some((_, value)) = decls
            .iter()
            .find(|(prop, _)| prop.eq_ignore_ascii_case(&name))
        {
            removed = value.clone();
        }
        decls.retain(|(prop, _)| !prop.eq_ignore_ascii_case(&name));
    });
    Ok(js_str(&removed))
}

fn get_property_value(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let name = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;

    // Parse the style attribute looking for the requested property.
    // This is a simplification: it does not consult the computed style.
    let doc = ctx.doc.borrow();
    let style_attr = doc
        .get_node(node_id)
        .and_then(|node| node.attr(blitz_dom::local_name!("style")))
        .unwrap_or_default();
    let value = style_attr
        .split(';')
        .filter_map(|decl| decl.split_once(':'))
        .find(|(prop, _)| prop.trim().eq_ignore_ascii_case(&name))
        .map(|(_, value)| value.trim().to_string())
        .unwrap_or_default();
    Ok(js_str(&value))
}
