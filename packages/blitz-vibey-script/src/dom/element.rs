//! The `Element` class: attributes, DOM properties (`value`, `checked`, ...),
//! `style`, `innerHTML` and friends.

use blitz_dom::{LocalName, NodeId, QualName, ScrollBehavior, ScrollLogicalPosition};
use boa_engine::class::ClassBuilder;
use boa_engine::object::{JsObject, ObjectInitializer};
use boa_engine::property::Attribute as PropAttribute;
use boa_engine::value::JsValue;
use boa_engine::{Context, Finalize, JsData, JsNativeError, JsResult, Trace, js_string};

use crate::shared::{
    ExtendLayer, Extended, from_chain, instance_accessor, instance_getter, instance_method,
    js_fn_ptr, native_fn_ptr,
};

use super::node::{append, prepend, replace_children};
use super::style::{CSSStyleDeclaration, StyleLayer};
use super::{
    dom_ctx, js_str, node_or_null, node_wrapper, this_node_id, to_rust_string, wrap_style_object,
};
use crate::state::DomCtx;

/// `Element` own block. All data lives in the `Node` layer; this layer only
/// contributes the element interface to the prototype chain.
#[derive(Debug, Default, Clone, Trace, Finalize, JsData)]
pub(crate) struct ElementLayer;

pub(crate) type Element = Extended<ElementLayer>;

impl ExtendLayer for ElementLayer {
    type Parent = super::node::NodeLayer;
    const CLASS_NAME: &'static str = "Element";
    fn define_members(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        let realm = class.context().realm().clone();
        let attr = PropAttribute::CONFIGURABLE | PropAttribute::NON_ENUMERABLE;

        instance_getter!(class, "tagName", js_fn_ptr!(tag_name, &realm), attr);
        instance_getter!(class, "localName", js_fn_ptr!(local_name, &realm), attr);
        instance_getter!(
            class,
            "namespaceURI",
            js_fn_ptr!(namespace_uri, &realm),
            attr
        );
        instance_accessor!(
            class,
            "id",
            js_fn_ptr!(get_id, &realm),
            js_fn_ptr!(set_id, &realm),
            attr
        );
        instance_accessor!(
            class,
            "className",
            js_fn_ptr!(get_class_name, &realm),
            js_fn_ptr!(set_class_name, &realm),
            attr
        );
        instance_accessor!(
            class,
            "value",
            js_fn_ptr!(get_value, &realm),
            js_fn_ptr!(set_value, &realm),
            attr
        );
        instance_accessor!(
            class,
            "checked",
            js_fn_ptr!(get_checked, &realm),
            js_fn_ptr!(set_checked, &realm),
            attr
        );
        instance_accessor!(
            class,
            "disabled",
            js_fn_ptr!(get_disabled, &realm),
            js_fn_ptr!(set_disabled, &realm),
            attr
        );
        instance_accessor!(
            class,
            "hidden",
            js_fn_ptr!(get_hidden, &realm),
            js_fn_ptr!(set_hidden, &realm),
            attr
        );
        instance_accessor!(
            class,
            "selectionStart",
            js_fn_ptr!(get_selection_start, &realm),
            js_fn_ptr!(set_selection_start, &realm),
            attr
        );
        instance_accessor!(
            class,
            "selectionEnd",
            js_fn_ptr!(get_selection_end, &realm),
            js_fn_ptr!(set_selection_end, &realm),
            attr
        );
        instance_method!(
            class,
            "setSelectionRange",
            2,
            native_fn_ptr!(set_selection_range)
        );
        instance_accessor!(
            class,
            "placeholder",
            js_fn_ptr!(get_placeholder, &realm),
            js_fn_ptr!(set_placeholder, &realm),
            attr
        );
        instance_accessor!(
            class,
            "type",
            js_fn_ptr!(get_type, &realm),
            js_fn_ptr!(set_type, &realm),
            attr
        );
        instance_accessor!(
            class,
            "autofocus",
            js_fn_ptr!(get_autofocus, &realm),
            js_fn_ptr!(set_autofocus, &realm),
            attr
        );
        instance_getter!(class, "style", js_fn_ptr!(get_style, &realm), attr);
        instance_accessor!(
            class,
            "innerHTML",
            js_fn_ptr!(get_inner_html, &realm),
            js_fn_ptr!(set_inner_html, &realm),
            attr
        );
        instance_getter!(class, "outerHTML", js_fn_ptr!(get_outer_html, &realm), attr);
        instance_getter!(class, "content", js_fn_ptr!(get_content, &realm), attr);
        instance_getter!(class, "children", js_fn_ptr!(children, &realm), attr);
        instance_getter!(
            class,
            "childElementCount",
            js_fn_ptr!(child_element_count, &realm),
            attr
        );
        instance_getter!(
            class,
            "firstElementChild",
            js_fn_ptr!(first_element_child, &realm),
            attr
        );
        instance_getter!(
            class,
            "lastElementChild",
            js_fn_ptr!(last_element_child, &realm),
            attr
        );
        instance_getter!(
            class,
            "nextElementSibling",
            js_fn_ptr!(next_element_sibling, &realm),
            attr
        );
        instance_getter!(
            class,
            "previousElementSibling",
            js_fn_ptr!(previous_element_sibling, &realm),
            attr
        );
        instance_getter!(class, "offsetWidth", js_fn_ptr!(offset_width, &realm), attr);
        instance_getter!(
            class,
            "offsetHeight",
            js_fn_ptr!(offset_height, &realm),
            attr
        );
        instance_getter!(class, "offsetLeft", js_fn_ptr!(offset_left, &realm), attr);
        instance_getter!(class, "offsetTop", js_fn_ptr!(offset_top, &realm), attr);
        instance_getter!(class, "clientWidth", js_fn_ptr!(client_width, &realm), attr);
        instance_getter!(
            class,
            "clientHeight",
            js_fn_ptr!(client_height, &realm),
            attr
        );
        instance_getter!(class, "scrollWidth", js_fn_ptr!(scroll_width, &realm), attr);
        instance_getter!(
            class,
            "scrollHeight",
            js_fn_ptr!(scroll_height, &realm),
            attr
        );
        instance_accessor!(
            class,
            "scrollTop",
            js_fn_ptr!(get_scroll_top, &realm),
            js_fn_ptr!(set_scroll_top, &realm),
            attr
        );
        instance_accessor!(
            class,
            "scrollLeft",
            js_fn_ptr!(get_scroll_left, &realm),
            js_fn_ptr!(set_scroll_left, &realm),
            attr
        );
        instance_method!(class, "scroll", 2, native_fn_ptr!(scroll_to_method));
        instance_method!(class, "scrollTo", 2, native_fn_ptr!(scroll_to_method));
        instance_method!(class, "scrollBy", 2, native_fn_ptr!(scroll_by_method));
        instance_method!(class, "scrollIntoView", 0, native_fn_ptr!(scroll_into_view));

        instance_method!(class, "getAttribute", 1, native_fn_ptr!(get_attribute));
        instance_method!(class, "setAttribute", 2, native_fn_ptr!(set_attribute));
        instance_method!(
            class,
            "removeAttribute",
            1,
            native_fn_ptr!(remove_attribute)
        );
        instance_method!(class, "hasAttribute", 1, native_fn_ptr!(has_attribute));
        instance_method!(class, "focus", 0, native_fn_ptr!(focus));
        instance_method!(class, "blur", 0, native_fn_ptr!(blur));
        instance_method!(
            class,
            "getBoundingClientRect",
            0,
            native_fn_ptr!(get_bounding_client_rect)
        );
        instance_method!(class, "getClientRects", 0, native_fn_ptr!(get_client_rects));
        // ParentNode mixin mutation helpers
        instance_method!(class, "append", 1, native_fn_ptr!(append));
        instance_method!(class, "prepend", 1, native_fn_ptr!(prepend));
        instance_method!(
            class,
            "replaceChildren",
            1,
            native_fn_ptr!(replace_children)
        );

        instance_method!(class, "querySelector", 1, native_fn_ptr!(query_selector));
        instance_method!(
            class,
            "querySelectorAll",
            1,
            native_fn_ptr!(query_selector_all)
        );
        instance_method!(class, "matches", 1, native_fn_ptr!(matches));
        instance_method!(class, "webkitMatchesSelector", 1, native_fn_ptr!(matches));
        instance_method!(class, "closest", 1, native_fn_ptr!(closest));
        instance_method!(
            class,
            "getElementsByTagName",
            1,
            native_fn_ptr!(get_elements_by_tag_name)
        );
        instance_method!(
            class,
            "getElementsByClassName",
            1,
            native_fn_ptr!(get_elements_by_class_name)
        );

        Ok(())
    }
}

/// Register the `Element` class and wire up the `Element -> Node` prototype chain.
pub(crate) fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<Element>()?;
    crate::shared::link_prototype::<Element>(context)?;
    Ok(())
}

/// Construct a `QualName` for an attribute (no namespace)
pub(crate) fn attr_name(local: &str) -> QualName {
    QualName::new(None, markup5ever::ns!(), LocalName::from(local))
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

fn children(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
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
    Ok(node_or_null(&ctx, child_id, context))
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
    Ok(node_or_null(&ctx, child_id, context))
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
    Ok(node_or_null(&ctx, sibling_id, context))
}

fn previous_element_sibling(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let sibling_id = element_sibling(&ctx.doc.borrow(), node_id, -1);
    Ok(node_or_null(&ctx, sibling_id, context))
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

fn get_hidden(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    Ok(JsValue::from(read_attr(&ctx, node_id, "hidden").is_some()))
}

fn set_hidden(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
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

// === Text input selection (`selectionStart`/`selectionEnd`) ===
//
// Backed by the text input's real editor selection. Offsets are in UTF-16 code
// units, per spec. Returns null for elements without a text editor. React
// snapshots these around controlled-input re-renders and restores the caret
// via `setSelectionRange()`.

/// Convert a byte offset in `text` to a UTF-16 code-unit offset
fn byte_to_utf16(text: &str, byte_offset: usize) -> usize {
    text[..byte_offset.min(text.len())]
        .chars()
        .map(char::len_utf16)
        .sum()
}

/// Convert a UTF-16 code-unit offset to a byte offset in `text` (clamped)
fn utf16_to_byte(text: &str, utf16_offset: usize) -> usize {
    let mut units = 0;
    for (byte_offset, ch) in text.char_indices() {
        if units >= utf16_offset {
            return byte_offset;
        }
        units += ch.len_utf16();
    }
    text.len()
}

/// The current selection of the node's text editor as UTF-16 offsets
fn selection_utf16_range(doc: &blitz_dom::BaseDocument, node_id: NodeId) -> Option<(usize, usize)> {
    let input = doc.get_node(node_id)?.element_data()?.text_input_data()?;
    let text = input.editor.raw_text();
    let range = input.editor.raw_selection().text_range();
    Some((
        byte_to_utf16(text, range.start),
        byte_to_utf16(text, range.end),
    ))
}

/// Set the node's text editor selection from UTF-16 offsets
fn set_selection_utf16_range(
    doc: &mut blitz_dom::BaseDocument,
    node_id: NodeId,
    start: usize,
    end: usize,
) {
    doc.with_text_input(node_id, |mut driver| {
        let text = driver.editor.raw_text().to_string();
        let start_byte = utf16_to_byte(&text, start);
        let end_byte = utf16_to_byte(&text, start.max(end));
        driver.select_byte_range(start_byte, end_byte);
    });
}

fn get_selection_start(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let mut doc = ctx.doc.borrow_mut();
    // The text editor is created during layout construction
    doc.resolve(0.0);
    Ok(match selection_utf16_range(&doc, node_id) {
        Some((start, _)) => JsValue::from(start as u32),
        None => JsValue::null(),
    })
}

fn get_selection_end(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let mut doc = ctx.doc.borrow_mut();
    // The text editor is created during layout construction
    doc.resolve(0.0);
    Ok(match selection_utf16_range(&doc, node_id) {
        Some((_, end)) => JsValue::from(end as u32),
        None => JsValue::null(),
    })
}

fn set_selection_start(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let start = args
        .first()
        .unwrap_or(&JsValue::undefined())
        .to_number(context)?
        .max(0.0) as usize;
    let mut doc = ctx.doc.borrow_mut();
    doc.resolve(0.0);
    if let Some((_, end)) = selection_utf16_range(&doc, node_id) {
        set_selection_utf16_range(&mut doc, node_id, start, end.max(start));
    }
    Ok(JsValue::undefined())
}

fn set_selection_end(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let end = args
        .first()
        .unwrap_or(&JsValue::undefined())
        .to_number(context)?
        .max(0.0) as usize;
    let mut doc = ctx.doc.borrow_mut();
    doc.resolve(0.0);
    if let Some((start, _)) = selection_utf16_range(&doc, node_id) {
        set_selection_utf16_range(&mut doc, node_id, start.min(end), end);
    }
    Ok(JsValue::undefined())
}

fn set_selection_range(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let start = args
        .first()
        .unwrap_or(&JsValue::undefined())
        .to_number(context)?
        .max(0.0) as usize;
    let end = args
        .get(1)
        .unwrap_or(&JsValue::undefined())
        .to_number(context)?
        .max(0.0) as usize;
    let mut doc = ctx.doc.borrow_mut();
    doc.resolve(0.0);
    set_selection_utf16_range(&mut doc, node_id, start, end);
    Ok(JsValue::undefined())
}

// === Style ===

fn get_style(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let node_id = this_node_id(this)?;
    let obj = from_chain!((CSSStyleDeclaration, context), StyleLayer { node_id })?;
    Ok(wrap_style_object(obj, context))
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

/// `HTMLTemplateElement.content`: the template's inert contents fragment.
/// Returns `undefined` for non-template elements.
fn get_content(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
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

// `offsetLeft`/`offsetTop`: the position of the element's border edge relative
// to the padding edge of its `offsetParent` (blitz-dom's `offset_top_left`
// implements the offsetParent resolution)

fn offset_left(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    layout_value(this, context, |node| node.offset_top_left().x.round())
}

fn offset_top(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    layout_value(this, context, |node| node.offset_top_left().y.round())
}

fn client_width(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    layout_value(this, context, |node| node.client_width().round())
}

fn client_height(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    layout_value(this, context, |node| node.client_height().round())
}

fn scroll_width(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    layout_value(this, context, |node| node.scroll_width().round())
}

fn scroll_height(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    layout_value(this, context, |node| node.scroll_height().round())
}

// === Scrolling ===

/// The current scroll offset of an element. The root element scrolls the
/// viewport (per the CSS overflow propagation rules), so its offset is the
/// viewport scroll offset.
fn current_scroll_offset(doc: &blitz_dom::BaseDocument, node_id: NodeId) -> blitz_dom::Point<f64> {
    if doc
        .try_root_element()
        .is_some_and(|root| root.id == node_id)
    {
        doc.viewport_scroll()
    } else {
        doc.get_node(node_id)
            .filter(|node| node.element_data().is_some())
            .map(|node| *node.scroll_offset())
            .unwrap_or(blitz_dom::Point { x: 0.0, y: 0.0 })
    }
}

/// Convert a JS value to a scroll coordinate: WebIDL `unrestricted double`,
/// with non-finite values normalized to 0 per the cssom-view spec.
fn scroll_coord(value: &JsValue, context: &mut Context) -> JsResult<f64> {
    let number = value.to_number(context)?;
    Ok(if number.is_finite() { number } else { 0.0 })
}

/// Parse a WebIDL `ScrollBehavior` enumeration value (`undefined` maps to the
/// default, "auto"; other invalid values throw a `TypeError`)
fn parse_scroll_behavior(value: &JsValue, context: &mut Context) -> JsResult<ScrollBehavior> {
    if value.is_undefined() {
        return Ok(ScrollBehavior::Auto);
    }
    let string = to_rust_string(value, context)?;
    match string.as_str() {
        "auto" => Ok(ScrollBehavior::Auto),
        "instant" => Ok(ScrollBehavior::Instant),
        "smooth" => Ok(ScrollBehavior::Smooth),
        _ => Err(JsNativeError::typ()
            .with_message(format!(
                "{string:?} is not a valid value for enumeration ScrollBehavior"
            ))
            .into()),
    }
}

/// Parsed arguments of the `scrollTo(x, y)` / `scrollTo(options)` overloads
/// shared by the `Element` and `Window` scroll methods. `None` coordinates
/// were not specified (and default to the current position for `scrollTo`,
/// or a zero delta for `scrollBy`).
pub(crate) struct ScrollToArgs {
    pub(crate) left: Option<f64>,
    pub(crate) top: Option<f64>,
    pub(crate) behavior: ScrollBehavior,
}

/// Parse the WebIDL `scrollTo(x, y)` / `scrollTo(options)` overloads: two or
/// more arguments are coordinates; a single argument must be a `ScrollToOptions`
/// dictionary (a single non-object argument throws a `TypeError`, per WebIDL
/// dictionary conversion).
pub(crate) fn parse_scroll_to_args(
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<ScrollToArgs> {
    if args.len() >= 2 {
        return Ok(ScrollToArgs {
            left: Some(scroll_coord(&args[0], context)?),
            top: Some(scroll_coord(&args[1], context)?),
            behavior: ScrollBehavior::Auto,
        });
    }
    match args.first() {
        None => Ok(ScrollToArgs {
            left: None,
            top: None,
            behavior: ScrollBehavior::Auto,
        }),
        Some(value) if value.is_null_or_undefined() => Ok(ScrollToArgs {
            left: None,
            top: None,
            behavior: ScrollBehavior::Auto,
        }),
        Some(value) => {
            let Some(options) = value.as_object() else {
                return Err(JsNativeError::typ()
                    .with_message("value cannot be converted to a ScrollToOptions dictionary")
                    .into());
            };
            let left = options.get(js_string!("left"), context)?;
            let left = (!left.is_undefined())
                .then(|| scroll_coord(&left, context))
                .transpose()?;
            let top = options.get(js_string!("top"), context)?;
            let top = (!top.is_undefined())
                .then(|| scroll_coord(&top, context))
                .transpose()?;
            let behavior = options.get(js_string!("behavior"), context)?;
            let behavior = parse_scroll_behavior(&behavior, context)?;
            Ok(ScrollToArgs {
                left,
                top,
                behavior,
            })
        }
    }
}

fn get_scroll_top(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let doc = ctx.doc.borrow();
    Ok(JsValue::from(current_scroll_offset(&doc, node_id).y))
}

fn get_scroll_left(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let doc = ctx.doc.borrow();
    Ok(JsValue::from(current_scroll_offset(&doc, node_id).x))
}

fn set_scroll_top(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = scroll_coord(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.resolve(0.0);
    let current = current_scroll_offset(&doc, node_id);
    doc.scroll_to(node_id, current.x, value, ScrollBehavior::Auto);
    Ok(JsValue::undefined())
}

fn set_scroll_left(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let value = scroll_coord(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.resolve(0.0);
    let current = current_scroll_offset(&doc, node_id);
    doc.scroll_to(node_id, value, current.y, ScrollBehavior::Auto);
    Ok(JsValue::undefined())
}

fn scroll_to_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let parsed = parse_scroll_to_args(args, context)?;
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.resolve(0.0);
    let current = current_scroll_offset(&doc, node_id);
    doc.scroll_to(
        node_id,
        parsed.left.unwrap_or(current.x),
        parsed.top.unwrap_or(current.y),
        parsed.behavior,
    );
    Ok(JsValue::undefined())
}

fn scroll_by_method(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let parsed = parse_scroll_to_args(args, context)?;
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.resolve(0.0);
    doc.scroll_by(
        node_id,
        parsed.left.unwrap_or(0.0),
        parsed.top.unwrap_or(0.0),
        parsed.behavior,
    );
    Ok(JsValue::undefined())
}

/// Parse a WebIDL `ScrollLogicalPosition` enumeration value (`undefined` maps
/// to the given default; other invalid values throw a `TypeError`)
fn parse_scroll_logical_position(
    value: &JsValue,
    default: ScrollLogicalPosition,
    context: &mut Context,
) -> JsResult<ScrollLogicalPosition> {
    if value.is_undefined() {
        return Ok(default);
    }
    let string = to_rust_string(value, context)?;
    match string.as_str() {
        "start" => Ok(ScrollLogicalPosition::Start),
        "center" => Ok(ScrollLogicalPosition::Center),
        "end" => Ok(ScrollLogicalPosition::End),
        "nearest" => Ok(ScrollLogicalPosition::Nearest),
        _ => Err(JsNativeError::typ()
            .with_message(format!(
                "{string:?} is not a valid value for enumeration ScrollLogicalPosition"
            ))
            .into()),
    }
}

fn scroll_into_view(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let arg = args.first().cloned().unwrap_or_default();
    // WebIDL `(boolean or ScrollIntoViewOptions)`: objects (and null/undefined)
    // convert to the options dictionary, everything else converts to a boolean
    // (`true` aligns to the start, `false` to the end).
    let (behavior, block, inline) = if let Some(options) = arg.as_object() {
        let behavior = options.get(js_string!("behavior"), context)?;
        let behavior = parse_scroll_behavior(&behavior, context)?;
        let block = options.get(js_string!("block"), context)?;
        let block = parse_scroll_logical_position(&block, ScrollLogicalPosition::Start, context)?;
        let inline = options.get(js_string!("inline"), context)?;
        let inline =
            parse_scroll_logical_position(&inline, ScrollLogicalPosition::Nearest, context)?;
        (behavior, block, inline)
    } else if arg.is_null_or_undefined() || arg.to_boolean() {
        (
            ScrollBehavior::Auto,
            ScrollLogicalPosition::Start,
            ScrollLogicalPosition::Nearest,
        )
    } else {
        (
            ScrollBehavior::Auto,
            ScrollLogicalPosition::End,
            ScrollLogicalPosition::Nearest,
        )
    };

    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.resolve(0.0);
    doc.scroll_into_view(node_id, behavior, block, inline);
    Ok(JsValue::undefined())
}

/// Build a `DOMRect`-shaped object
fn make_rect_object(context: &mut Context, x: f64, y: f64, width: f64, height: f64) -> JsObject {
    ObjectInitializer::new(context)
        .property(js_string!("x"), x, PropAttribute::all())
        .property(js_string!("y"), y, PropAttribute::all())
        .property(js_string!("width"), width, PropAttribute::all())
        .property(js_string!("height"), height, PropAttribute::all())
        .property(js_string!("left"), x, PropAttribute::all())
        .property(js_string!("top"), y, PropAttribute::all())
        .property(js_string!("right"), x + width, PropAttribute::all())
        .property(js_string!("bottom"), y + height, PropAttribute::all())
        .build()
}

/// Does the node generate any boxes? (`getClientRects()` returns an empty list
/// for boxless nodes, e.g. `display: none` or detached elements)
fn node_has_boxes(doc: &blitz_dom::BaseDocument, node_id: NodeId) -> bool {
    doc.get_node(node_id).is_some_and(|node| node.has_boxes())
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
    Ok(make_rect_object(context, x, y, width, height).into())
}

fn get_client_rects(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    ctx.doc.borrow_mut().resolve(0.0);

    // One rect per box fragment: a single border-box rect for nodes with their
    // own layout box, one rect per line box for non-atomic inline elements,
    // and an empty list for boxless (e.g. `display: none`) elements
    let rects = {
        let doc = ctx.doc.borrow();
        if node_has_boxes(&doc, node_id) {
            doc.node_client_rects(node_id)
        } else {
            Vec::new()
        }
    };

    let rect_objects: Vec<JsValue> = rects
        .into_iter()
        .map(|rect| make_rect_object(context, rect.x, rect.y, rect.width, rect.height).into())
        .collect();
    Ok(boa_engine::object::builtins::JsArray::from_iter(rect_objects, context).into())
}

// === Scoped selector queries ===

/// A `DOMException` for an unparseable selector, per the spec for
/// `querySelector`/`matches`/`closest` and friends
fn invalid_selector_error(context: &mut Context, selector: &str) -> boa_engine::JsError {
    super::dom_exception(
        context,
        "SyntaxError",
        &format!("{selector:?} is not a valid selector"),
    )
}

fn query_selector(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let selector = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;

    let result = ctx.doc.borrow().query_selector_in(node_id, &selector);
    let result = match result {
        Ok(result) => result,
        Err(_) => return Err(invalid_selector_error(context, &selector)),
    };
    Ok(node_or_null(&ctx, result, context))
}

fn query_selector_all(
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

fn matches(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let selector = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;

    match ctx.doc.borrow().matches_selector(node_id, &selector) {
        Ok(matches) => Ok(JsValue::from(matches)),
        Err(_) => Err(invalid_selector_error(context, &selector)),
    }
}

fn closest(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let selector = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;

    let result = ctx.doc.borrow().closest(node_id, &selector);
    match result {
        Ok(result) => Ok(node_or_null(&ctx, result, context)),
        Err(_) => Err(invalid_selector_error(context, &selector)),
    }
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
