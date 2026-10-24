//! Element text, HTML serialization, template content, and style access.

use crate::dom::{dom_ctx, this_node_id};
use crate::dom::{js_str, node_wrapper, to_rust_string};
use boa_engine::object::JsObject;
use boa_engine::{Context, JsResult, JsValue};

pub(super) fn get_inner_text(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.resolve(0.0);
    let text = doc
        .get_node(node_id)
        .map(crate::inner_text::inner_text)
        .unwrap_or_default();
    Ok(js_str(&text))
}

// === Style ===

pub(super) fn get_style(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let proto = ctx.state.borrow().protos().style.clone();
    let obj = JsObject::from_proto_and_data(Some(proto), crate::dom::NodeRef { node_id });
    Ok(crate::dom::wrap_style_object(obj, context))
}

/// `HTMLStyleElement.sheet` / `HTMLLinkElement.sheet`: the `CSSStyleSheet`
/// object (built by the JS bootstrap) for the element's stylesheet, or `null`
pub(super) fn get_sheet(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    this_node_id(this)?;
    let sheet = crate::dom::call_js_helper(
        "__blitz_sheet_for_node",
        std::slice::from_ref(this),
        context,
    )?;
    Ok(if sheet.is_undefined() {
        JsValue::null()
    } else {
        sheet
    })
}

// === innerHTML / outerHTML ===

pub(super) fn get_inner_html(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let doc = ctx.doc.borrow();
    let mut html = String::new();
    if let Some(node) = doc.get_node(node_id) {
        for child_id in &node.children {
            if let Some(child) = doc.get_node(*child_id) {
                child.write_outer_html(&mut html);
            }
        }
    }
    Ok(js_str(&html))
}

pub(super) fn set_inner_html(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let html = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;

    let mut doc = ctx.doc.borrow_mut();
    let mut mutr = doc.mutate();
    // Detach (rather than drop) any existing children so that JS wrappers
    // referencing them remain valid.
    for child_id in mutr.child_ids(node_id) {
        mutr.remove_node(child_id);
    }
    mutr.set_inner_html(node_id, &html);
    Ok(JsValue::undefined())
}

/// `HTMLTemplateElement.content`: the template's inert contents fragment.
/// Returns `undefined` for non-template elements.
pub(super) fn get_content(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;

    let is_template = ctx
        .doc
        .borrow()
        .get_node(node_id)
        .and_then(|node| node.element_data())
        .is_some_and(|element| element.name.local == blitz_dom::local_name!("template"));
    if !is_template {
        return Ok(JsValue::undefined());
    }

    // Lazily create the contents fragment for templates created by script
    let contents_id = ctx.doc.borrow_mut().mutate().template_contents(node_id);
    Ok(node_wrapper(&ctx, contents_id, context).into())
}

pub(super) fn get_outer_html(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let doc = ctx.doc.borrow();
    let html = doc
        .get_node(node_id)
        .map(|node| node.outer_html())
        .unwrap_or_default();
    Ok(js_str(&html))
}
