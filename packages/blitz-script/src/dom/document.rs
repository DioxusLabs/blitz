//! The `Document` prototype: node creation and lookup.

use blitz_dom::NodeId;
use boa_engine::object::JsObject;
use boa_engine::object::builtins::JsArray;
use boa_engine::value::JsValue;
use boa_engine::{Context, JsResult};

use super::{
    define_accessor, define_method, dom_ctx, node_or_null, node_wrapper, qual_name, qual_name_ns,
    this_node_id, to_rust_string,
};

pub(crate) fn init_document_proto(proto: &JsObject, context: &mut Context) {
    define_accessor(
        proto,
        "documentElement",
        Some(document_element),
        None,
        context,
    );
    define_accessor(proto, "body", Some(body), None, context);
    define_accessor(proto, "head", Some(head), None, context);
    define_accessor(proto, "activeElement", Some(active_element), None, context);
    define_accessor(proto, "defaultView", Some(default_view), None, context);
    define_accessor(proto, "title", Some(title), None, context);
    define_accessor(proto, "readyState", Some(ready_state), None, context);
    define_accessor(
        proto,
        "childElementCount",
        Some(super::element::child_element_count),
        None,
        context,
    );
    define_accessor(
        proto,
        "firstElementChild",
        Some(super::element::first_element_child),
        None,
        context,
    );
    define_accessor(
        proto,
        "lastElementChild",
        Some(super::element::last_element_child),
        None,
        context,
    );

    define_method(proto, "createElement", 1, create_element, context);
    define_method(proto, "createElementNS", 2, create_element_ns, context);
    define_method(proto, "createTextNode", 1, create_text_node, context);
    define_method(proto, "createComment", 1, create_comment, context);
    define_method(
        proto,
        "createDocumentFragment",
        0,
        create_document_fragment,
        context,
    );
    define_method(proto, "getElementById", 1, get_element_by_id, context);
    define_method(
        proto,
        "getElementsByTagName",
        1,
        get_elements_by_tag_name,
        context,
    );
    define_method(
        proto,
        "getElementsByClassName",
        1,
        get_elements_by_class_name,
        context,
    );
    define_method(proto, "querySelector", 1, query_selector, context);
    define_method(proto, "querySelectorAll", 1, query_selector_all, context);
    define_method(proto, "elementFromPoint", 2, element_from_point, context);
    define_method(proto, "elementsFromPoint", 2, elements_from_point, context);

    // ParentNode mixin mutation helpers
    define_method(proto, "append", 1, super::node::append, context);
    define_method(proto, "prepend", 1, super::node::prepend, context);
    define_method(
        proto,
        "replaceChildren",
        1,
        super::node::replace_children,
        context,
    );
}

fn document_element(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this)?;
    let root_id = ctx.doc.borrow().try_root_element().map(|root| root.id);
    Ok(node_or_null(&ctx, root_id, context))
}

fn body(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this)?;
    let body_id = ctx.doc.borrow().find_body_node().map(|node| node.id);
    Ok(node_or_null(&ctx, body_id, context))
}

fn head(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this)?;
    let head_id = ctx.doc.borrow().find_head_node().map(|node| node.id);
    Ok(node_or_null(&ctx, head_id, context))
}

fn active_element(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this)?;
    let focus_id = ctx.doc.borrow().get_focussed_node_id();
    Ok(node_or_null(&ctx, focus_id, context))
}

fn default_view(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let _ = this_node_id(this)?;
    Ok(context.global_object().into())
}

fn title(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this)?;
    let title = ctx
        .doc
        .borrow()
        .find_title_node()
        .map(|node| node.text_content())
        .unwrap_or_default();
    Ok(super::js_str(&title))
}

fn ready_state(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this)?;
    let ready_state = ctx.state.borrow().ready_state;
    Ok(super::js_str(ready_state.as_str()))
}

// === Node creation ===

fn create_element(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this)?;
    let tag = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?
        .to_ascii_lowercase();
    let node_id = {
        let mut doc = ctx.doc.borrow_mut();
        doc.mutate().create_element(qual_name(&tag), Vec::new())
    };
    Ok(node_wrapper(&ctx, node_id, context).into())
}

fn create_element_ns(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this)?;
    let ns = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let tag = to_rust_string(args.get(1).unwrap_or(&JsValue::undefined()), context)?;
    let node_id = {
        let mut doc = ctx.doc.borrow_mut();
        doc.mutate()
            .create_element(qual_name_ns(&tag, &ns), Vec::new())
    };
    Ok(node_wrapper(&ctx, node_id, context).into())
}

fn create_text_node(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this)?;
    let text = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let node_id = {
        let mut doc = ctx.doc.borrow_mut();
        doc.mutate().create_text_node(&text)
    };
    Ok(node_wrapper(&ctx, node_id, context).into())
}

fn create_comment(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this)?;
    let text = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let node_id = {
        let mut doc = ctx.doc.borrow_mut();
        doc.mutate().create_comment_node(&text)
    };
    Ok(node_wrapper(&ctx, node_id, context).into())
}

fn create_document_fragment(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this)?;
    let node_id = {
        let mut doc = ctx.doc.borrow_mut();
        doc.mutate()
            .create_element(qual_name("#document-fragment"), Vec::new())
    };
    Ok(node_wrapper(&ctx, node_id, context).into())
}

// === Lookup ===

fn get_element_by_id(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this)?;
    let id = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let node_id = ctx.doc.borrow().get_element_by_id(&id);
    Ok(node_or_null(&ctx, node_id, context))
}

fn get_elements_by_tag_name(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this)?;
    let tag = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?
        .to_ascii_lowercase();
    let match_all = tag == "*";

    let root_id = ctx.doc.borrow().root_node().id;
    let matches = super::collect_matching_descendants(&ctx.doc.borrow(), root_id, |element| {
        match_all || &*element.name.local == tag.as_str()
    });

    let wrappers: Vec<JsValue> = matches
        .into_iter()
        .map(|match_id| node_wrapper(&ctx, match_id, context).into())
        .collect();
    Ok(JsArray::from_iter(wrappers, context).into())
}

fn get_elements_by_class_name(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this)?;
    let class_arg = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let class_names: Vec<&str> = class_arg.split_whitespace().collect();

    let matches = if class_names.is_empty() {
        Vec::new()
    } else {
        let root_id = ctx.doc.borrow().root_node().id;
        super::collect_matching_descendants(&ctx.doc.borrow(), root_id, |element| {
            super::matches_class_names(element, &class_names)
        })
    };

    let wrappers: Vec<JsValue> = matches
        .into_iter()
        .map(|match_id| node_wrapper(&ctx, match_id, context).into())
        .collect();
    Ok(JsArray::from_iter(wrappers, context).into())
}

// === Hit testing ===

fn point_args(args: &[JsValue], context: &mut Context) -> JsResult<(f32, f32)> {
    let x = args
        .first()
        .unwrap_or(&JsValue::undefined())
        .to_number(context)? as f32;
    let y = args
        .get(1)
        .unwrap_or(&JsValue::undefined())
        .to_number(context)? as f32;
    Ok((x, y))
}

fn element_from_point(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this)?;
    let (x, y) = point_args(args, context)?;

    let element_id = {
        let mut doc = ctx.doc.borrow_mut();
        // Hit testing consults layout, so make sure it is up to date
        doc.resolve(0.0);
        doc.element_from_point(x, y)
    };
    Ok(node_or_null(&ctx, element_id, context))
}

fn elements_from_point(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this)?;
    let (x, y) = point_args(args, context)?;

    let element_ids: Vec<NodeId> = {
        let mut doc = ctx.doc.borrow_mut();
        doc.resolve(0.0);
        doc.elements_from_point(x, y)
    };

    let wrappers: Vec<JsValue> = element_ids
        .into_iter()
        .map(|node_id| node_wrapper(&ctx, node_id, context).into())
        .collect();
    Ok(JsArray::from_iter(wrappers, context).into())
}

/// A `DOMException` for an unparseable selector, per the spec for
/// `querySelector` and friends
fn invalid_selector_error(context: &mut Context, selector: &str) -> boa_engine::JsError {
    super::dom_exception(
        context,
        "SyntaxError",
        &format!("{selector:?} is not a valid selector"),
    )
}

fn query_selector(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this)?;
    let selector = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let node_id = match ctx.doc.borrow().query_selector(&selector) {
        Ok(node_id) => node_id,
        Err(_) => return Err(invalid_selector_error(context, &selector)),
    };
    Ok(node_or_null(&ctx, node_id, context))
}

fn query_selector_all(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this)?;
    let selector = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let matches = match ctx.doc.borrow().query_selector_all(&selector) {
        Ok(matches) => matches,
        Err(_) => return Err(invalid_selector_error(context, &selector)),
    };
    let wrappers: Vec<JsValue> = matches
        .into_iter()
        .map(|match_id| node_wrapper(&ctx, match_id, context).into())
        .collect();
    Ok(JsArray::from_iter(wrappers, context).into())
}
