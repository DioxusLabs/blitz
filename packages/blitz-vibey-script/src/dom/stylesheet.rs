//! Native helpers backing the CSSOM stylesheet API (`document.styleSheets`,
//! `CSSStyleSheet`, `CSSRuleList`, `CSSRule`, ...).
//!
//! The JS-facing objects are built in the runtime bootstrap script on top of
//! these `__blitz_sheet_*` functions. Stylesheets are identified by their
//! owner node (`<style>` / `<link>`) and rules by a path of indices through
//! nested rule lists (see `blitz_dom`'s CSSOM support).

use blitz_dom::{CssRuleInfo, CssomError, NodeId};
use boa_engine::object::ObjectInitializer;
use boa_engine::object::builtins::JsArray;
use boa_engine::property::Attribute;
use boa_engine::value::JsValue;
use boa_engine::{Context, JsNativeError, JsResult, JsString, js_string};

use super::{dom_ctx, dom_exception, js_str, node_id_of_value, node_wrapper, to_rust_string};

pub(crate) fn register(context: &mut Context) {
    let fns: &[(&str, usize, super::NativeFnPtr)] = &[
        ("__blitz_stylesheet_owner_nodes", 0, owner_nodes),
        ("__blitz_node_has_stylesheet", 1, node_has_stylesheet),
        ("__blitz_sheet_rule_count", 2, rule_count),
        ("__blitz_sheet_rule_info", 2, rule_info),
        ("__blitz_sheet_insert_rule", 4, insert_rule),
        ("__blitz_sheet_delete_rule", 3, delete_rule),
        ("__blitz_sheet_style_css_text", 2, style_css_text),
        ("__blitz_sheet_style_set_css_text", 3, style_set_css_text),
        (
            "__blitz_sheet_style_property_names",
            2,
            style_property_names,
        ),
        ("__blitz_sheet_style_get", 3, style_get_property),
        ("__blitz_sheet_style_set", 5, style_set_property),
        ("__blitz_sheet_style_remove", 3, style_remove_property),
    ];
    for (name, length, body) in fns {
        context
            .register_global_callable(
                JsString::from(*name),
                *length,
                boa_engine::NativeFunction::from_fn_ptr(*body),
            )
            .expect("failed to register CSSOM function");
    }
}

fn arg(args: &[JsValue], index: usize) -> JsValue {
    args.get(index).cloned().unwrap_or_default()
}

fn node_arg(args: &[JsValue], index: usize) -> JsResult<NodeId> {
    node_id_of_value(&arg(args, index)).ok_or_else(|| {
        JsNativeError::typ()
            .with_message("expected a DOM node")
            .into()
    })
}

/// Read a rule path (a JS array of indices) argument
fn path_arg(args: &[JsValue], index: usize, context: &mut Context) -> JsResult<Vec<usize>> {
    let value = arg(args, index);
    let Some(obj) = value.as_object() else {
        return Ok(Vec::new());
    };
    let array = JsArray::from_object(obj)?;
    let len = array.length(context)?;
    let mut path = Vec::with_capacity(len as usize);
    for i in 0..len {
        let n = array.get(i, context)?.to_number(context)?;
        path.push(n.max(0.0) as usize);
    }
    Ok(path)
}

fn index_arg(args: &[JsValue], index: usize, context: &mut Context) -> JsResult<usize> {
    let n = arg(args, index).to_number(context)?;
    if n.is_nan() {
        return Ok(0);
    }
    Ok(n.max(0.0) as usize)
}

fn string_arg(args: &[JsValue], index: usize, context: &mut Context) -> JsResult<String> {
    to_rust_string(&arg(args, index), context)
}

fn cssom_error(err: CssomError, context: &mut Context, what: &str) -> boa_engine::JsError {
    dom_exception(context, err.dom_exception_name(), what)
}

fn owner_nodes(_: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let owners = ctx.doc.borrow().stylesheet_owner_nodes();
    let wrappers: Vec<JsValue> = owners
        .into_iter()
        .map(|node_id| node_wrapper(&ctx, node_id, context).into())
        .collect();
    Ok(JsArray::from_iter(wrappers, context).into())
}

fn node_has_stylesheet(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let Some(node_id) = node_id_of_value(&arg(args, 0)) else {
        return Ok(JsValue::from(false));
    };
    Ok(JsValue::from(ctx.doc.borrow().node_has_stylesheet(node_id)))
}

fn rule_count(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = node_arg(args, 0)?;
    let path = path_arg(args, 1, context)?;
    let count = ctx.doc.borrow().stylesheet_rule_count(node_id, &path);
    Ok(match count {
        Some(count) => JsValue::from(count as f64),
        None => JsValue::from(-1),
    })
}

fn rule_info_object(info: CssRuleInfo, context: &mut Context) -> JsValue {
    let mut attrs = ObjectInitializer::new(context);
    for (name, value) in &info.attributes {
        attrs.property(JsString::from(*name), js_str(value), Attribute::all());
    }
    let attrs = attrs.build();
    ObjectInitializer::new(context)
        .property(
            js_string!("interface"),
            js_str(info.interface),
            Attribute::all(),
        )
        .property(
            js_string!("type"),
            JsValue::from(info.rule_type),
            Attribute::all(),
        )
        .property(
            js_string!("cssText"),
            js_str(&info.css_text),
            Attribute::all(),
        )
        .property(
            js_string!("hasChildRules"),
            JsValue::from(info.has_child_rules),
            Attribute::all(),
        )
        .property(
            js_string!("hasStyle"),
            JsValue::from(info.has_style),
            Attribute::all(),
        )
        .property(js_string!("attrs"), attrs, Attribute::all())
        .build()
        .into()
}

fn rule_info(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = node_arg(args, 0)?;
    let path = path_arg(args, 1, context)?;
    let info = ctx.doc.borrow().stylesheet_rule_info(node_id, &path);
    Ok(match info {
        Some(info) => rule_info_object(info, context),
        None => JsValue::null(),
    })
}

fn insert_rule(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = node_arg(args, 0)?;
    let path = path_arg(args, 1, context)?;
    let rule = string_arg(args, 2, context)?;
    let index = index_arg(args, 3, context)?;
    let result = ctx
        .doc
        .borrow_mut()
        .stylesheet_insert_rule(node_id, &path, &rule, index);
    match result {
        Ok(index) => Ok(JsValue::from(index as f64)),
        Err(err) => Err(cssom_error(
            err,
            context,
            &format!("Failed to insert rule '{rule}'"),
        )),
    }
}

fn delete_rule(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = node_arg(args, 0)?;
    let path = path_arg(args, 1, context)?;
    let index = index_arg(args, 2, context)?;
    let result = ctx
        .doc
        .borrow_mut()
        .stylesheet_delete_rule(node_id, &path, index);
    match result {
        Ok(()) => Ok(JsValue::undefined()),
        Err(err) => Err(cssom_error(
            err,
            context,
            &format!("Failed to delete rule at index {index}"),
        )),
    }
}

fn style_css_text(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = node_arg(args, 0)?;
    let path = path_arg(args, 1, context)?;
    let css = ctx
        .doc
        .borrow()
        .stylesheet_rule_style_css_text(node_id, &path)
        .unwrap_or_default();
    Ok(js_str(&css))
}

fn style_set_css_text(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = node_arg(args, 0)?;
    let path = path_arg(args, 1, context)?;
    let css = string_arg(args, 2, context)?;
    // Errors (e.g. a rule whose declarations cannot be replaced) are ignored:
    // `cssText` assignment never throws per CSSOM
    let _ = ctx
        .doc
        .borrow_mut()
        .stylesheet_rule_style_set_css_text(node_id, &path, &css);
    Ok(JsValue::undefined())
}

fn style_property_names(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = node_arg(args, 0)?;
    let path = path_arg(args, 1, context)?;
    let names = ctx
        .doc
        .borrow()
        .stylesheet_rule_style_property_names(node_id, &path)
        .unwrap_or_default();
    let values: Vec<JsValue> = names.iter().map(|name| js_str(name)).collect();
    Ok(JsArray::from_iter(values, context).into())
}

fn style_get_property(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = node_arg(args, 0)?;
    let path = path_arg(args, 1, context)?;
    let property = string_arg(args, 2, context)?;
    let (value, important) = ctx
        .doc
        .borrow()
        .stylesheet_rule_style_get_property(node_id, &path, &property)
        .unwrap_or_default();
    Ok(JsArray::from_iter([js_str(&value), JsValue::from(important)], context).into())
}

fn style_set_property(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = node_arg(args, 0)?;
    let path = path_arg(args, 1, context)?;
    let property = string_arg(args, 2, context)?;
    let value = string_arg(args, 3, context)?;
    let important = arg(args, 4).to_boolean();
    // Invalid declarations are ignored per CSSOM; a missing rule is a no-op
    let _ = ctx
        .doc
        .borrow_mut()
        .stylesheet_rule_style_set_property(node_id, &path, &property, &value, important);
    Ok(JsValue::undefined())
}

fn style_remove_property(
    _: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = node_arg(args, 0)?;
    let path = path_arg(args, 1, context)?;
    let property = string_arg(args, 2, context)?;
    let removed = ctx
        .doc
        .borrow_mut()
        .stylesheet_rule_style_remove_property(node_id, &path, &property)
        .unwrap_or_default();
    Ok(js_str(&removed))
}
