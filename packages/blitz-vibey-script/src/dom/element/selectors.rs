//! Scoped element selector queries and descendant collections.

use crate::dom::{dom_ctx, this_node_id};
use crate::dom::{node_wrapper, to_rust_string};
use boa_engine::{Context, JsResult, JsValue};

// === Scoped selector queries ===

/// A `DOMException` for an unparseable selector, per the spec for
/// `querySelector`/`matches`/`closest` and friends
fn invalid_selector_error(context: &mut Context, selector: &str) -> boa_engine::JsError {
    crate::dom::dom_exception(
        context,
        "SyntaxError",
        &format!("{selector:?} is not a valid selector"),
    )
}

pub(super) fn query_selector(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let selector = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;

    let result = ctx.doc.borrow().query_selector_in(node_id, &selector);
    let result = match result {
        Ok(result) => result,
        Err(_) => return Err(invalid_selector_error(context, &selector)),
    };
    Ok(crate::dom::node_or_null(&ctx, result, context))
}

pub(super) fn query_selector_all(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let selector = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;

    let matches = ctx.doc.borrow().query_selector_all_in(node_id, &selector);
    let matches = match matches {
        Ok(matches) => matches,
        Err(_) => return Err(invalid_selector_error(context, &selector)),
    };
    let wrappers: Vec<JsValue> = matches
        .into_iter()
        .map(|match_id| node_wrapper(&ctx, match_id, context).into())
        .collect();
    Ok(boa_engine::object::builtins::JsArray::from_iter(wrappers, context).into())
}

pub(super) fn matches(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let selector = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;

    match ctx.doc.borrow().matches_selector(node_id, &selector) {
        Ok(matches) => Ok(JsValue::from(matches)),
        Err(_) => Err(invalid_selector_error(context, &selector)),
    }
}

pub(super) fn closest(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let selector = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;

    let result = ctx.doc.borrow().closest(node_id, &selector);
    match result {
        Ok(result) => Ok(crate::dom::node_or_null(&ctx, result, context)),
        Err(_) => Err(invalid_selector_error(context, &selector)),
    }
}

pub(super) fn get_elements_by_tag_name(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let tag = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?
        .to_ascii_lowercase();
    let match_all = tag == "*";

    let matches = crate::dom::collect_matching_descendants(&ctx.doc.borrow(), node_id, |element| {
        match_all || &*element.name.local == tag.as_str()
    });

    let wrappers: Vec<JsValue> = matches
        .into_iter()
        .map(|match_id| node_wrapper(&ctx, match_id, context).into())
        .collect();
    Ok(boa_engine::object::builtins::JsArray::from_iter(wrappers, context).into())
}

pub(super) fn get_elements_by_class_name(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let class_arg = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let class_names: Vec<&str> = class_arg.split_whitespace().collect();

    let matches = if class_names.is_empty() {
        Vec::new()
    } else {
        crate::dom::collect_matching_descendants(&ctx.doc.borrow(), node_id, |element| {
            crate::dom::matches_class_names(element, &class_names)
        })
    };

    let wrappers: Vec<JsValue> = matches
        .into_iter()
        .map(|match_id| node_wrapper(&ctx, match_id, context).into())
        .collect();
    Ok(boa_engine::object::builtins::JsArray::from_iter(wrappers, context).into())
}
