//! The `Element` prototype: attributes, DOM properties (`value`, `checked`, ...),
//! `style`, `innerHTML` and friends.

use blitz_dom::{LocalName, NodeId, QualName};
use boa_engine::object::{JsObject, ObjectInitializer};
use boa_engine::property::Attribute as PropAttribute;
use boa_engine::value::JsValue;
use boa_engine::{Context, JsResult, js_string};

use super::{
    define_accessor, define_method, dom_ctx, js_str, node_wrapper, this_node_id, to_rust_string,
};
use crate::state::DomCtx;

/// Construct a `QualName` for an attribute (no namespace)
pub(crate) fn attr_name(local: &str) -> QualName {
    QualName::new(None, markup5ever::ns!(), LocalName::from(local))
}

pub(crate) fn init_element_proto(proto: &JsObject, context: &mut Context) {
    define_accessor(proto, "tagName", Some(tag_name), None, context);
    define_accessor(proto, "localName", Some(local_name), None, context);
    define_accessor(proto, "namespaceURI", Some(namespace_uri), None, context);
    define_accessor(proto, "id", Some(get_id), Some(set_id), context);
    define_accessor(
        proto,
        "className",
        Some(get_class_name),
        Some(set_class_name),
        context,
    );
    define_accessor(proto, "value", Some(get_value), Some(set_value), context);
    define_accessor(
        proto,
        "checked",
        Some(get_checked),
        Some(set_checked),
        context,
    );
    define_accessor(
        proto,
        "disabled",
        Some(get_disabled),
        Some(set_disabled),
        context,
    );
    define_accessor(
        proto,
        "placeholder",
        Some(get_placeholder),
        Some(set_placeholder),
        context,
    );
    define_accessor(proto, "type", Some(get_type), Some(set_type), context);
    define_accessor(
        proto,
        "autofocus",
        Some(get_autofocus),
        Some(set_autofocus),
        context,
    );
    define_accessor(proto, "style", Some(get_style), None, context);
    define_accessor(
        proto,
        "innerHTML",
        Some(get_inner_html),
        Some(set_inner_html),
        context,
    );
    define_accessor(proto, "outerHTML", Some(get_outer_html), None, context);
    define_accessor(proto, "children", Some(children), None, context);
    define_accessor(proto, "offsetWidth", Some(offset_width), None, context);
    define_accessor(proto, "offsetHeight", Some(offset_height), None, context);
    define_accessor(proto, "offsetLeft", Some(offset_left), None, context);
    define_accessor(proto, "offsetTop", Some(offset_top), None, context);
    define_accessor(proto, "clientWidth", Some(client_width), None, context);
    define_accessor(proto, "clientHeight", Some(client_height), None, context);
    define_accessor(proto, "scrollWidth", Some(scroll_width), None, context);
    define_accessor(proto, "scrollHeight", Some(scroll_height), None, context);

    define_method(proto, "getAttribute", 1, get_attribute, context);
    define_method(proto, "setAttribute", 2, set_attribute, context);
    define_method(proto, "removeAttribute", 1, remove_attribute, context);
    define_method(proto, "hasAttribute", 1, has_attribute, context);
    define_method(proto, "focus", 0, focus, context);
    define_method(proto, "blur", 0, blur, context);
    define_method(
        proto,
        "getBoundingClientRect",
        0,
        get_bounding_client_rect,
        context,
    );
    define_method(proto, "querySelector", 1, query_selector, context);
    define_method(proto, "querySelectorAll", 1, query_selector_all, context);
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
}

// === Attribute helpers ===

fn read_attr(ctx: &DomCtx, node_id: NodeId, name: &str) -> Option<String> {
    let doc = ctx.doc.borrow();
    let node = doc.get_node(node_id)?;
    let element = node.element_data()?;
    element
        .attrs()
        .iter()
        .find(|attr| &*attr.name.local == name)
        .map(|attr| attr.value.clone())
}

fn write_attr(ctx: &DomCtx, node_id: NodeId, name: &str, value: &str) {
    let mut doc = ctx.doc.borrow_mut();
    doc.mutate().set_attribute(node_id, attr_name(name), value);
}

fn clear_attr(ctx: &DomCtx, node_id: NodeId, name: &str) {
    let mut doc = ctx.doc.borrow_mut();
    doc.mutate().clear_attribute(node_id, attr_name(name));
}

fn attr_getter(name: &str, this: &JsValue, context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    Ok(js_str(&read_attr(&ctx, node_id, name).unwrap_or_default()))
}

fn attr_setter(
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

// === Basic element info ===

fn tag_name(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let doc = ctx.doc.borrow();
    let name = doc
        .get_node(node_id)
        .and_then(|node| node.element_data())
        .map(|element| element.name.local.to_uppercase())
        .unwrap_or_default();
    Ok(js_str(&name))
}

fn local_name(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let doc = ctx.doc.borrow();
    let name = doc
        .get_node(node_id)
        .and_then(|node| node.element_data())
        .map(|element| element.name.local.to_string())
        .unwrap_or_default();
    Ok(js_str(&name))
}

fn namespace_uri(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let doc = ctx.doc.borrow();
    let ns = doc
        .get_node(node_id)
        .and_then(|node| node.element_data())
        .map(|element| element.name.ns.to_string())
        .unwrap_or_default();
    Ok(js_str(&ns))
}

fn children(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let child_ids: Vec<NodeId> = {
        let doc = ctx.doc.borrow();
        doc.get_node(node_id)
            .map(|node| {
                node.children
                    .iter()
                    .copied()
                    .filter(|child_id| {
                        doc.get_node(*child_id)
                            .is_some_and(|child| child.is_element())
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let wrappers: Vec<JsValue> = child_ids
        .into_iter()
        .map(|child_id| node_wrapper(&ctx, child_id, context).into())
        .collect();
    Ok(boa_engine::object::builtins::JsArray::from_iter(wrappers, context).into())
}

// === Attributes ===

fn get_attribute(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let name = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?
        .to_ascii_lowercase();
    match read_attr(&ctx, node_id, &name) {
        Some(value) => Ok(js_str(&value)),
        None => Ok(JsValue::null()),
    }
}

fn set_attribute(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let name = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?
        .to_ascii_lowercase();
    let value = to_rust_string(args.get(1).unwrap_or(&JsValue::undefined()), context)?;
    write_attr(&ctx, node_id, &name, &value);
    Ok(JsValue::undefined())
}

fn remove_attribute(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let name = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?
        .to_ascii_lowercase();
    clear_attr(&ctx, node_id, &name);
    Ok(JsValue::undefined())
}

fn has_attribute(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let name = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?
        .to_ascii_lowercase();
    Ok(JsValue::from(read_attr(&ctx, node_id, &name).is_some()))
}

// === Reflected DOM properties ===

fn get_id(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let _ = args;
    attr_getter("id", this, context)
}
fn set_id(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    attr_setter("id", this, args, context)
}

fn get_class_name(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let _ = args;
    attr_getter("class", this, context)
}
fn set_class_name(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    attr_setter("class", this, args, context)
}

fn get_placeholder(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let _ = args;
    attr_getter("placeholder", this, context)
}
fn set_placeholder(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    attr_setter("placeholder", this, args, context)
}

fn get_type(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let _ = args;
    attr_getter("type", this, context)
}
fn set_type(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    attr_setter("type", this, args, context)
}

fn get_autofocus(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    Ok(JsValue::from(
        read_attr(&ctx, node_id, "autofocus").is_some(),
    ))
}
fn set_autofocus(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let value = args.first().map(JsValue::to_boolean).unwrap_or(false);
    if value {
        // blitz-dom's autofocus handling expects the value "true"
        write_attr(&ctx, node_id, "autofocus", "true");
    } else {
        clear_attr(&ctx, node_id, "autofocus");
    }
    Ok(JsValue::undefined())
}

fn get_value(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let value = {
        let doc = ctx.doc.borrow();
        doc.get_node(node_id)
            .and_then(|node| node.element_data())
            .map(|element| match element.text_input_data() {
                Some(input_data) => input_data.editor.raw_text().to_string(),
                None => element
                    .attrs()
                    .iter()
                    .find(|attr| &*attr.name.local == "value")
                    .map(|attr| attr.value.clone())
                    .unwrap_or_default(),
            })
            .unwrap_or_default()
    };
    Ok(js_str(&value))
}

fn set_value(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    attr_setter("value", this, args, context)
}

fn get_checked(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let checked = {
        let doc = ctx.doc.borrow();
        doc.get_node(node_id)
            .and_then(|node| node.element_data())
            .map(|element| {
                element
                    .checkbox_input_checked()
                    .unwrap_or_else(|| element.attr(blitz_dom::local_name!("checked")).is_some())
            })
            .unwrap_or(false)
    };
    Ok(JsValue::from(checked))
}

fn set_checked(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let checked = args.first().map(JsValue::to_boolean).unwrap_or(false);
    // blitz-dom's checked handling parses the value as a boolean
    write_attr(
        &ctx,
        node_id,
        "checked",
        if checked { "true" } else { "false" },
    );
    Ok(JsValue::undefined())
}

fn get_disabled(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    Ok(JsValue::from(
        read_attr(&ctx, node_id, "disabled").is_some(),
    ))
}

fn set_disabled(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let disabled = args.first().map(JsValue::to_boolean).unwrap_or(false);
    if disabled {
        write_attr(&ctx, node_id, "disabled", "");
    } else {
        clear_attr(&ctx, node_id, "disabled");
    }
    Ok(JsValue::undefined())
}

// === Style ===

fn get_style(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let proto = ctx.state.borrow().protos().style.clone();
    let obj = JsObject::from_proto_and_data(Some(proto), super::NodeRef { node_id });
    Ok(super::wrap_style_object(obj, context))
}

// === innerHTML / outerHTML ===

fn get_inner_html(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
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

fn set_inner_html(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
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

fn get_outer_html(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let doc = ctx.doc.borrow();
    let html = doc
        .get_node(node_id)
        .map(|node| node.outer_html())
        .unwrap_or_default();
    Ok(js_str(&html))
}

// === Focus ===

fn focus(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    ctx.doc.borrow_mut().set_focus_to(node_id);
    Ok(JsValue::undefined())
}

fn blur(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this)?;
    ctx.doc.borrow_mut().clear_focus();
    Ok(JsValue::undefined())
}

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

fn offset_width(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    layout_value(this, context, |node| node.final_layout().size.width.round())
}

fn offset_height(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    layout_value(this, context, |node| {
        node.final_layout().size.height.round()
    })
}

fn offset_left(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    layout_value(this, context, |node| {
        let parent_border = match node.parent {
            Some(parent_id) => node.with(parent_id).final_layout().border.left,
            None => 0.0,
        };
        (node.final_layout().location.x - parent_border).round()
    })
}

fn offset_top(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    layout_value(this, context, |node| {
        let parent_border = match node.parent {
            Some(parent_id) => node.with(parent_id).final_layout().border.top,
            None => 0.0,
        };
        (node.final_layout().location.y - parent_border).round()
    })
}

fn client_width_of(node: &blitz_dom::Node) -> f32 {
    let layout = node.final_layout();
    layout.size.width - layout.border.left - layout.border.right - layout.scrollbar_size.width
}

fn client_height_of(node: &blitz_dom::Node) -> f32 {
    let layout = node.final_layout();
    layout.size.height - layout.border.top - layout.border.bottom - layout.scrollbar_size.height
}

fn client_width(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    layout_value(this, context, |node| client_width_of(node).round())
}

fn client_height(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    layout_value(this, context, |node| client_height_of(node).round())
}

fn scroll_width(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    layout_value(this, context, |node| {
        client_width_of(node)
            .max(node.final_layout().content_size.width)
            .round()
    })
}

fn scroll_height(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    layout_value(this, context, |node| {
        client_height_of(node)
            .max(node.final_layout().content_size.height)
            .round()
    })
}

fn get_bounding_client_rect(
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
    let object = ObjectInitializer::new(context)
        .property(js_string!("x"), x, PropAttribute::all())
        .property(js_string!("y"), y, PropAttribute::all())
        .property(js_string!("width"), width, PropAttribute::all())
        .property(js_string!("height"), height, PropAttribute::all())
        .property(js_string!("left"), x, PropAttribute::all())
        .property(js_string!("top"), y, PropAttribute::all())
        .property(js_string!("right"), x + width, PropAttribute::all())
        .property(js_string!("bottom"), y + height, PropAttribute::all())
        .build();
    Ok(object.into())
}

// === Scoped selector queries ===

fn is_descendant_of(doc: &blitz_dom::BaseDocument, node_id: NodeId, ancestor_id: NodeId) -> bool {
    let mut current = doc.get_node(node_id).and_then(|node| node.parent);
    while let Some(id) = current {
        if id == ancestor_id {
            return true;
        }
        current = doc.get_node(id).and_then(|node| node.parent);
    }
    false
}

fn query_selector(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let selector = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;

    let result = {
        let doc = ctx.doc.borrow();
        doc.query_selector_all(&selector).ok().and_then(|matches| {
            matches
                .into_iter()
                .find(|match_id| is_descendant_of(&doc, *match_id, node_id))
        })
    };
    Ok(super::node_or_null(&ctx, result, context))
}

fn query_selector_all(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let selector = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;

    let matches: Vec<NodeId> = {
        let doc = ctx.doc.borrow();
        doc.query_selector_all(&selector)
            .map(|matches| {
                matches
                    .into_iter()
                    .filter(|match_id| is_descendant_of(&doc, *match_id, node_id))
                    .collect()
            })
            .unwrap_or_default()
    };
    let wrappers: Vec<JsValue> = matches
        .into_iter()
        .map(|match_id| node_wrapper(&ctx, match_id, context).into())
        .collect();
    Ok(boa_engine::object::builtins::JsArray::from_iter(wrappers, context).into())
}

fn get_elements_by_tag_name(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let tag = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?
        .to_ascii_lowercase();
    let match_all = tag == "*";

    let matches = super::collect_matching_descendants(&ctx.doc.borrow(), node_id, |element| {
        match_all || &*element.name.local == tag.as_str()
    });

    let wrappers: Vec<JsValue> = matches
        .into_iter()
        .map(|match_id| node_wrapper(&ctx, match_id, context).into())
        .collect();
    Ok(boa_engine::object::builtins::JsArray::from_iter(wrappers, context).into())
}

fn get_elements_by_class_name(
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
        super::collect_matching_descendants(&ctx.doc.borrow(), node_id, |element| {
            super::matches_class_names(element, &class_names)
        })
    };

    let wrappers: Vec<JsValue> = matches
        .into_iter()
        .map(|match_id| node_wrapper(&ctx, match_id, context).into())
        .collect();
    Ok(boa_engine::object::builtins::JsArray::from_iter(wrappers, context).into())
}
