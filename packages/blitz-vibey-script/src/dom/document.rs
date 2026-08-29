//! The `Document` class: node creation and lookup.

use blitz_dom::NodeId;
use boa_engine::class::ClassBuilder;
use boa_engine::object::builtins::JsArray;
use boa_engine::{Context, Finalize, JsData, JsResult, JsValue, Trace};

use crate::shared::{
    Constructed, ExtendLayer, Extended, Super, instance_getter, instance_method, js_fn_ptr,
    native_error, native_fn_ptr,
};

use super::{
    dom_ctx, js_str, node_or_null, node_wrapper, qual_name, qual_name_ns, this_node_id,
    to_rust_string,
};
use super::node::{append, prepend, replace_children};

/// `Document` own block. All data lives in the `Node` layer; this layer only
/// contributes the document interface to the prototype chain.
#[derive(Debug, Default, Clone, Trace, Finalize, JsData)]
pub(crate) struct DocumentLayer;

pub(crate) type Document = Extended<DocumentLayer>;

impl ExtendLayer for DocumentLayer {
    type Parent = super::node::NodeLayer;
    const CLASS_NAME: &'static str = "Document";

    fn build(
        _args: &[JsValue],
        _ctx: &mut Context,
        _sup: Super<'_, Self::Parent>,
    ) -> JsResult<Constructed<Self>> {
        Err(native_error!(typ, "Illegal constructor"))
    }

    fn define_members(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        use boa_engine::property::Attribute;
        let realm = class.context().realm().clone();
        let attr = Attribute::CONFIGURABLE | Attribute::NON_ENUMERABLE;

        instance_getter!(class, "documentElement", js_fn_ptr!(document_element, &realm), attr);
        instance_getter!(class, "body", js_fn_ptr!(body, &realm), attr);
        instance_getter!(class, "scrollingElement", js_fn_ptr!(scrolling_element, &realm), attr);
        instance_getter!(class, "head", js_fn_ptr!(head, &realm), attr);
        instance_getter!(class, "activeElement", js_fn_ptr!(active_element, &realm), attr);
        instance_getter!(class, "defaultView", js_fn_ptr!(default_view, &realm), attr);
        instance_getter!(class, "title", js_fn_ptr!(title, &realm), attr);
        instance_getter!(class, "readyState", js_fn_ptr!(ready_state, &realm), attr);
        instance_getter!(
            class,
            "childElementCount",
            js_fn_ptr!(super::element::child_element_count, &realm),
            attr
        );
        instance_getter!(
            class,
            "firstElementChild",
            js_fn_ptr!(super::element::first_element_child, &realm),
            attr
        );
        instance_getter!(
            class,
            "lastElementChild",
            js_fn_ptr!(super::element::last_element_child, &realm),
            attr
        );

        instance_method!(class, "createElement", 1, native_fn_ptr!(create_element));
        instance_method!(class, "createElementNS", 2, native_fn_ptr!(create_element_ns));
        instance_method!(class, "createTextNode", 1, native_fn_ptr!(create_text_node));
        instance_method!(class, "createComment", 1, native_fn_ptr!(create_comment));
        instance_method!(class, "createDocumentFragment", 0, native_fn_ptr!(create_document_fragment));
        instance_method!(class, "getElementById", 1, native_fn_ptr!(get_element_by_id));
        instance_method!(class, "getElementsByTagName", 1, native_fn_ptr!(get_elements_by_tag_name));
        instance_method!(class, "getElementsByClassName", 1, native_fn_ptr!(get_elements_by_class_name));
        instance_method!(class, "querySelector", 1, native_fn_ptr!(query_selector));
        instance_method!(class, "querySelectorAll", 1, native_fn_ptr!(query_selector_all));
        instance_method!(class, "elementFromPoint", 2, native_fn_ptr!(element_from_point));
        instance_method!(class, "elementsFromPoint", 2, native_fn_ptr!(elements_from_point));

        // ParentNode mixin mutation helpers
        instance_method!(class, "append", 1, native_fn_ptr!(append));
        instance_method!(class, "prepend", 1, native_fn_ptr!(prepend));
        instance_method!(class, "replaceChildren", 1, native_fn_ptr!(replace_children));

        Ok(())
    }
}

/// Register the `Document` class and wire up the `Document -> Node` prototype chain.
pub(crate) fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<Document>()?;
    crate::shared::link_prototype::<Document>(context)?;
    Ok(())
}

fn document_element(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this, context)?;
    let root_id = ctx.doc.borrow().try_root_element().map(|root| root.id);
    Ok(node_or_null(&ctx, root_id, context))
}

/// `document.scrollingElement`: blitz documents are always in no-quirks mode,
/// where the scrolling element is the document element.
fn scrolling_element(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    document_element(this, args, context)
}

fn body(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this, context)?;
    let body_id = ctx.doc.borrow().find_body_node().map(|node| node.id);
    Ok(node_or_null(&ctx, body_id, context))
}

fn head(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this, context)?;
    let head_id = ctx.doc.borrow().find_head_node().map(|node| node.id);
    Ok(node_or_null(&ctx, head_id, context))
}

fn active_element(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this, context)?;
    let focus_id = ctx.doc.borrow().get_focussed_node_id();
    Ok(node_or_null(&ctx, focus_id, context))
}

fn default_view(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let _ = this_node_id(this, context)?;
    Ok(context.global_object().into())
}

fn title(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this, context)?;
    let title = ctx
        .doc
        .borrow()
        .find_title_node()
        .map(|node| node.text_content())
        .unwrap_or_default();
    Ok(js_str(&title))
}

fn ready_state(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this, context)?;
    let ready_state = ctx.state.borrow().ready_state;
    Ok(js_str(ready_state.as_str()))
}

// === Node creation ===

fn create_element(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this, context)?;
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
    let _ = this_node_id(this, context)?;
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
    let _ = this_node_id(this, context)?;
    let text = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let node_id = {
        let mut doc = ctx.doc.borrow_mut();
        doc.mutate().create_text_node(&text)
    };
    Ok(node_wrapper(&ctx, node_id, context).into())
}

fn create_comment(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this, context)?;
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
    let _ = this_node_id(this, context)?;
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
    let _ = this_node_id(this, context)?;
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
    let _ = this_node_id(this, context)?;
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
    let _ = this_node_id(this, context)?;
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
    let _ = this_node_id(this, context)?;
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
    let _ = this_node_id(this, context)?;
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
    let _ = this_node_id(this, context)?;
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
    let _ = this_node_id(this, context)?;
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
