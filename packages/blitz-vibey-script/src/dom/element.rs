//! Element prototype installation and tree navigation.

use blitz_dom::NodeId;
use boa_engine::object::JsObject;
use boa_engine::{Context, JsResult, JsValue};

use super::{define_accessor, define_method, dom_ctx, js_str, node_wrapper, this_node_id};

mod attributes;
mod form_controls;
mod geometry;
mod markup;
mod scrolling;
mod selectors;

pub(crate) use attributes::{attr_name, read_attr, write_attr};
pub(crate) use scrolling::parse_scroll_to_args;

pub(crate) fn init_element_proto(proto: &JsObject, context: &mut Context) {
    define_accessor(proto, "tagName", Some(tag_name), None, context);
    define_accessor(proto, "localName", Some(local_name), None, context);
    define_accessor(proto, "namespaceURI", Some(namespace_uri), None, context);
    define_accessor(
        proto,
        "id",
        Some(attributes::get_id),
        Some(attributes::set_id),
        context,
    );
    define_accessor(
        proto,
        "className",
        Some(attributes::get_class_name),
        Some(attributes::set_class_name),
        context,
    );
    define_accessor(
        proto,
        "value",
        Some(form_controls::get_value),
        Some(form_controls::set_value),
        context,
    );
    define_accessor(
        proto,
        "checked",
        Some(form_controls::get_checked),
        Some(form_controls::set_checked),
        context,
    );
    define_accessor(
        proto,
        "disabled",
        Some(attributes::get_disabled),
        Some(attributes::set_disabled),
        context,
    );
    define_accessor(
        proto,
        "open",
        Some(attributes::get_open),
        Some(attributes::set_open),
        context,
    );
    define_accessor(
        proto,
        "hidden",
        Some(attributes::get_hidden),
        Some(attributes::set_hidden),
        context,
    );
    define_accessor(
        proto,
        "width",
        Some(attributes::get_width),
        Some(attributes::set_width),
        context,
    );
    define_accessor(
        proto,
        "height",
        Some(attributes::get_height),
        Some(attributes::set_height),
        context,
    );
    define_accessor(
        proto,
        "selectionStart",
        Some(form_controls::get_selection_start),
        Some(form_controls::set_selection_start),
        context,
    );
    define_accessor(
        proto,
        "selectionEnd",
        Some(form_controls::get_selection_end),
        Some(form_controls::set_selection_end),
        context,
    );
    define_method(
        proto,
        "setSelectionRange",
        2,
        form_controls::set_selection_range,
        context,
    );
    define_accessor(
        proto,
        "placeholder",
        Some(form_controls::get_placeholder),
        Some(form_controls::set_placeholder),
        context,
    );
    define_accessor(
        proto,
        "type",
        Some(form_controls::get_type),
        Some(form_controls::set_type),
        context,
    );
    define_accessor(
        proto,
        "autofocus",
        Some(form_controls::get_autofocus),
        Some(form_controls::set_autofocus),
        context,
    );
    // `style` is `[PutForwards=cssText]`: assignment sets `style.cssText`
    define_accessor(
        proto,
        "style",
        Some(markup::get_style),
        Some(super::style::set_css_text),
        context,
    );
    define_accessor(proto, "sheet", Some(markup::get_sheet), None, context);
    define_accessor(
        proto,
        "innerHTML",
        Some(markup::get_inner_html),
        Some(markup::set_inner_html),
        context,
    );
    define_accessor(
        proto,
        "outerHTML",
        Some(markup::get_outer_html),
        None,
        context,
    );
    // The setter is approximated with `textContent` semantics (no `<br>` insertion)
    define_accessor(
        proto,
        "innerText",
        Some(markup::get_inner_text),
        Some(super::node::set_text_content),
        context,
    );
    // The `outerText` getter is the same as `innerText`'s
    define_accessor(
        proto,
        "outerText",
        Some(markup::get_inner_text),
        None,
        context,
    );
    define_accessor(proto, "content", Some(markup::get_content), None, context);
    define_accessor(proto, "children", Some(children), None, context);
    define_accessor(
        proto,
        "childElementCount",
        Some(child_element_count),
        None,
        context,
    );
    define_accessor(
        proto,
        "firstElementChild",
        Some(first_element_child),
        None,
        context,
    );
    define_accessor(
        proto,
        "lastElementChild",
        Some(last_element_child),
        None,
        context,
    );
    define_accessor(
        proto,
        "nextElementSibling",
        Some(next_element_sibling),
        None,
        context,
    );
    define_accessor(
        proto,
        "previousElementSibling",
        Some(previous_element_sibling),
        None,
        context,
    );
    define_accessor(
        proto,
        "offsetWidth",
        Some(geometry::offset_width),
        None,
        context,
    );
    define_accessor(
        proto,
        "offsetHeight",
        Some(geometry::offset_height),
        None,
        context,
    );
    define_accessor(
        proto,
        "offsetLeft",
        Some(geometry::offset_left),
        None,
        context,
    );
    define_accessor(
        proto,
        "offsetTop",
        Some(geometry::offset_top),
        None,
        context,
    );
    define_accessor(
        proto,
        "clientWidth",
        Some(geometry::client_width),
        None,
        context,
    );
    define_accessor(
        proto,
        "clientTop",
        Some(geometry::client_top),
        None,
        context,
    );
    define_accessor(
        proto,
        "clientLeft",
        Some(geometry::client_left),
        None,
        context,
    );
    define_accessor(
        proto,
        "offsetParent",
        Some(geometry::offset_parent),
        None,
        context,
    );
    define_accessor(
        proto,
        "clientHeight",
        Some(geometry::client_height),
        None,
        context,
    );
    define_accessor(
        proto,
        "scrollWidth",
        Some(geometry::scroll_width),
        None,
        context,
    );
    define_accessor(
        proto,
        "scrollHeight",
        Some(geometry::scroll_height),
        None,
        context,
    );
    define_accessor(
        proto,
        "scrollTop",
        Some(scrolling::get_scroll_top),
        Some(scrolling::set_scroll_top),
        context,
    );
    define_accessor(
        proto,
        "scrollLeft",
        Some(scrolling::get_scroll_left),
        Some(scrolling::set_scroll_left),
        context,
    );
    define_method(proto, "scroll", 2, scrolling::scroll_to_method, context);
    define_method(proto, "scrollTo", 2, scrolling::scroll_to_method, context);
    define_method(proto, "scrollBy", 2, scrolling::scroll_by_method, context);
    define_method(
        proto,
        "scrollIntoView",
        0,
        scrolling::scroll_into_view,
        context,
    );

    define_method(proto, "getAttribute", 1, attributes::get_attribute, context);
    define_method(proto, "setAttribute", 2, attributes::set_attribute, context);
    define_method(
        proto,
        "removeAttribute",
        1,
        attributes::remove_attribute,
        context,
    );
    define_method(proto, "hasAttribute", 1, attributes::has_attribute, context);
    define_method(
        proto,
        "getAttributeNames",
        0,
        attributes::get_attribute_names,
        context,
    );
    define_accessor(
        proto,
        "attributes",
        Some(attributes::attributes),
        None,
        context,
    );
    define_method(proto, "focus", 0, form_controls::focus, context);
    define_method(proto, "blur", 0, form_controls::blur, context);
    define_method(
        proto,
        "getBoundingClientRect",
        0,
        geometry::get_bounding_client_rect,
        context,
    );
    define_method(
        proto,
        "getClientRects",
        0,
        geometry::get_client_rects,
        context,
    );
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

    define_method(
        proto,
        "querySelector",
        1,
        selectors::query_selector,
        context,
    );
    define_method(
        proto,
        "querySelectorAll",
        1,
        selectors::query_selector_all,
        context,
    );
    define_method(proto, "matches", 1, selectors::matches, context);
    define_method(
        proto,
        "webkitMatchesSelector",
        1,
        selectors::matches,
        context,
    );
    define_method(proto, "closest", 1, selectors::closest, context);
    define_method(
        proto,
        "getElementsByTagName",
        1,
        selectors::get_elements_by_tag_name,
        context,
    );
    define_method(
        proto,
        "getElementsByClassName",
        1,
        selectors::get_elements_by_class_name,
        context,
    );
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

/// The ids of the node's element children (in tree order)
pub(crate) fn element_child_ids(doc: &blitz_dom::BaseDocument, node_id: NodeId) -> Vec<NodeId> {
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
}

pub(crate) fn children(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let child_ids = element_child_ids(&ctx.doc.borrow(), node_id);
    let wrappers: Vec<JsValue> = child_ids
        .into_iter()
        .map(|child_id| node_wrapper(&ctx, child_id, context).into())
        .collect();
    Ok(boa_engine::object::builtins::JsArray::from_iter(wrappers, context).into())
}

pub(crate) fn child_element_count(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let count = element_child_ids(&ctx.doc.borrow(), node_id).len();
    Ok(JsValue::from(count as f64))
}

pub(crate) fn first_element_child(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let child_id = element_child_ids(&ctx.doc.borrow(), node_id)
        .first()
        .copied();
    Ok(super::node_or_null(&ctx, child_id, context))
}

pub(crate) fn last_element_child(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let child_id = element_child_ids(&ctx.doc.borrow(), node_id)
        .last()
        .copied();
    Ok(super::node_or_null(&ctx, child_id, context))
}

/// Find the nearest element sibling in the direction given by `offset`
/// (+1 = next, -1 = previous)
fn element_sibling(
    doc: &blitz_dom::BaseDocument,
    node_id: NodeId,
    offset: isize,
) -> Option<NodeId> {
    let node = doc.get_node(node_id)?;
    let parent = doc.get_node(node.parent?)?;
    let mut index = parent.index_of_child(node_id)?;
    loop {
        index = index.checked_add_signed(offset)?;
        let sibling_id = *parent.children.get(index)?;
        if doc
            .get_node(sibling_id)
            .is_some_and(|node| node.is_element())
        {
            return Some(sibling_id);
        }
    }
}

fn next_element_sibling(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let sibling_id = element_sibling(&ctx.doc.borrow(), node_id, 1);
    Ok(super::node_or_null(&ctx, sibling_id, context))
}

fn previous_element_sibling(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let sibling_id = element_sibling(&ctx.doc.borrow(), node_id, -1);
    Ok(super::node_or_null(&ctx, sibling_id, context))
}
