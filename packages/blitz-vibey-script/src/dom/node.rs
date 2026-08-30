//! The `Node` class: tree structure, tree mutation, text content.

use blitz_dom::NodeId;
use blitz_dom::node::NodeData;
use boa_engine::class::ClassBuilder;
use boa_engine::object::builtins::JsArray;
use boa_engine::property::Attribute;
use boa_engine::{Context, Finalize, JsData, JsResult, JsValue, Trace};

use crate::shared::{
    ExtendLayer, Extended, instance_accessor, instance_getter, instance_method, js_fn_ptr,
    native_error, native_fn_ptr,
};
use crate::state::DomCtx;

use super::{
    dom_ctx, js_str, node_id_of_value, node_or_null, node_wrapper, this_node_id, to_rust_string,
};

// ── Layer ────────────────────────────────────────────────────────────

/// `Node` own block: the wrapped blitz-dom node id.
#[derive(Debug, Default, Clone, Trace, Finalize, JsData)]
pub(crate) struct NodeLayer {
    #[unsafe_ignore_trace]
    pub node_id: NodeId,
}

pub(crate) type Node = Extended<NodeLayer>;

impl ExtendLayer for NodeLayer {
    type Parent = crate::events::EventTargetLayer;
    const CLASS_NAME: &'static str = "Node";

    fn define_members(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        let realm = class.context().realm().clone();
        let attr = Attribute::CONFIGURABLE | Attribute::NON_ENUMERABLE;

        instance_getter!(class, "nodeType", js_fn_ptr!(node_type, &realm), attr);
        instance_getter!(class, "nodeName", js_fn_ptr!(node_name, &realm), attr);
        instance_getter!(class, "parentNode", js_fn_ptr!(parent_node, &realm), attr);
        instance_getter!(
            class,
            "parentElement",
            js_fn_ptr!(parent_node, &realm),
            attr
        );
        instance_getter!(class, "childNodes", js_fn_ptr!(child_nodes, &realm), attr);
        instance_getter!(class, "firstChild", js_fn_ptr!(first_child, &realm), attr);
        instance_getter!(class, "lastChild", js_fn_ptr!(last_child, &realm), attr);
        instance_getter!(
            class,
            "previousSibling",
            js_fn_ptr!(previous_sibling, &realm),
            attr
        );
        instance_getter!(class, "nextSibling", js_fn_ptr!(next_sibling, &realm), attr);
        instance_getter!(class, "isConnected", js_fn_ptr!(is_connected, &realm), attr);
        instance_getter!(
            class,
            "ownerDocument",
            js_fn_ptr!(owner_document, &realm),
            attr
        );
        instance_accessor!(
            class,
            "textContent",
            js_fn_ptr!(text_content, &realm),
            js_fn_ptr!(set_text_content, &realm),
            attr
        );
        instance_accessor!(
            class,
            "nodeValue",
            js_fn_ptr!(node_value, &realm),
            js_fn_ptr!(set_node_value, &realm),
            attr
        );

        instance_method!(class, "appendChild", 1, native_fn_ptr!(append_child));
        instance_method!(class, "insertBefore", 2, native_fn_ptr!(insert_before));
        instance_method!(class, "removeChild", 1, native_fn_ptr!(remove_child));
        instance_method!(class, "replaceChild", 2, native_fn_ptr!(replace_child));
        instance_method!(class, "remove", 0, native_fn_ptr!(remove));
        // ChildNode mixin
        instance_method!(class, "before", 1, native_fn_ptr!(before));
        instance_method!(class, "after", 1, native_fn_ptr!(after));
        instance_method!(class, "replaceWith", 1, native_fn_ptr!(replace_with));
        instance_method!(class, "hasChildNodes", 0, native_fn_ptr!(has_child_nodes));
        instance_method!(class, "contains", 1, native_fn_ptr!(contains));
        instance_method!(class, "cloneNode", 1, native_fn_ptr!(clone_node));

        Ok(())
    }
}

/// Register the `Node` class and wire up the `Node -> EventTarget` prototype chain.
pub(crate) fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<Node>()?;
    crate::shared::link_prototype::<Node>(context)?;
    Ok(())
}

// === Read-only tree structure ===

fn node_type(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let doc = ctx.doc.borrow();
    let node_type = match doc.get_node(node_id).map(|node| &node.data) {
        Some(NodeData::Document(_)) => 9,
        Some(NodeData::Element(_)) if is_fragment(&doc, node_id) => 11,
        Some(NodeData::Element(_)) | Some(NodeData::AnonymousBlock(_)) => 1,
        Some(NodeData::Text(_)) => 3,
        Some(NodeData::Comment { .. }) => 8,
        None => 0,
    };
    Ok(JsValue::from(node_type))
}

fn node_name(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let doc = ctx.doc.borrow();
    let name = match doc.get_node(node_id).map(|node| &node.data) {
        Some(NodeData::Document(_)) => "#document".to_string(),
        // Fragments' special `#document-fragment` name is not uppercased
        Some(NodeData::Element(data)) if is_fragment(&doc, node_id) => data.name.local.to_string(),
        Some(NodeData::Element(data)) | Some(NodeData::AnonymousBlock(data)) => {
            data.name.local.to_uppercase()
        }
        Some(NodeData::Text(_)) => "#text".to_string(),
        Some(NodeData::Comment { .. }) => "#comment".to_string(),
        None => String::new(),
    };
    Ok(js_str(&name))
}

fn parent_node(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let parent_id = ctx
        .doc
        .borrow()
        .get_node(node_id)
        .and_then(|node| node.parent);
    Ok(node_or_null(&ctx, parent_id, context))
}

fn child_ids(ctx: &DomCtx, node_id: NodeId) -> Vec<NodeId> {
    ctx.doc
        .borrow()
        .get_node(node_id)
        .map(|node| node.children.to_vec())
        .unwrap_or_default()
}

fn child_nodes(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let children = child_ids(&ctx, node_id);
    let wrappers: Vec<JsValue> = children
        .into_iter()
        .map(|child_id| node_wrapper(&ctx, child_id, context).into())
        .collect();
    Ok(JsArray::from_iter(wrappers, context).into())
}

fn first_child(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let child_id = ctx
        .doc
        .borrow()
        .get_node(node_id)
        .and_then(|node| node.children.first().copied());
    Ok(node_or_null(&ctx, child_id, context))
}

fn last_child(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let child_id = ctx
        .doc
        .borrow()
        .get_node(node_id)
        .and_then(|node| node.children.last().copied());
    Ok(node_or_null(&ctx, child_id, context))
}

fn sibling(ctx: &DomCtx, node_id: NodeId, offset: isize) -> Option<NodeId> {
    let doc = ctx.doc.borrow();
    let node = doc.get_node(node_id)?;
    let parent = doc.get_node(node.parent?)?;
    let index = parent.index_of_child(node_id)?;
    let sibling_index = index.checked_add_signed(offset)?;
    parent.children.get(sibling_index).copied()
}

fn previous_sibling(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let sibling_id = sibling(&ctx, node_id, -1);
    Ok(node_or_null(&ctx, sibling_id, context))
}

fn next_sibling(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let sibling_id = sibling(&ctx, node_id, 1);
    Ok(node_or_null(&ctx, sibling_id, context))
}

fn is_connected(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let connected = ctx
        .doc
        .borrow()
        .get_node(node_id)
        .is_some_and(|node| node.flags.is_in_document());
    Ok(JsValue::from(connected))
}

fn owner_document(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _ = this_node_id(this)?;
    let root_id = ctx.doc.borrow().root_node().id;
    Ok(node_or_null(&ctx, Some(root_id), context))
}

// === Text content ===

fn text_content(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let doc = ctx.doc.borrow();
    let text = doc
        .get_node(node_id)
        .map(|node| node.text_content())
        .unwrap_or_default();
    Ok(js_str(&text))
}

fn set_text_content(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let text = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;

    let is_text_like = {
        let doc = ctx.doc.borrow();
        matches!(
            doc.get_node(node_id).map(|node| &node.data),
            Some(NodeData::Text(_)) | Some(NodeData::Comment { .. })
        )
    };

    let mut doc = ctx.doc.borrow_mut();
    let mut mutr = doc.mutate();
    if is_text_like {
        mutr.set_node_text(node_id, &text);
    } else {
        // Detach (rather than drop) any existing children so that JS wrappers
        // referencing them remain valid.
        for child_id in mutr.child_ids(node_id) {
            mutr.remove_node(child_id);
        }
        if !text.is_empty() {
            let text_id = mutr.create_text_node(&text);
            mutr.append_children(node_id, &[text_id]);
        }
    }
    Ok(JsValue::undefined())
}

pub(crate) fn node_value(
    this: &JsValue,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let doc = ctx.doc.borrow();
    match doc.get_node(node_id).map(|node| &node.data) {
        Some(NodeData::Text(data)) => Ok(js_str(&data.content)),
        Some(NodeData::Comment { .. }) => Ok(js_str("")),
        _ => Ok(JsValue::null()),
    }
}

pub(crate) fn set_node_value(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let text = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.mutate().set_node_text(node_id, &text);
    Ok(JsValue::undefined())
}

// === Tree mutation ===

fn arg_node_id(args: &[JsValue], index: usize) -> JsResult<NodeId> {
    args.get(index)
        .and_then(node_id_of_value)
        .ok_or_else(|| native_error!(typ, "argument is not a DOM node"))
}

fn append_child(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let parent_id = this_node_id(this)?;
    let child_id = arg_node_id(args, 0)?;
    // Inserting a DocumentFragment moves its children
    let node_ids = insertable_node_ids(&ctx, child_id);

    let mut doc = ctx.doc.borrow_mut();
    let mut mutr = doc.mutate();
    // Detach from any current parent first. This also makes "move to end of
    // same parent" operations behave correctly.
    detach_all(&mut mutr, &node_ids);
    mutr.append_children(parent_id, &node_ids);
    drop(mutr);
    drop(doc);

    Ok(args[0].clone())
}

fn insert_before(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let parent_id = this_node_id(this)?;
    let new_id = arg_node_id(args, 0)?;

    // A null/undefined reference node means "append"
    let ref_arg = args.get(1).cloned().unwrap_or_default();
    let ref_id = if ref_arg.is_null_or_undefined() {
        None
    } else {
        Some(arg_node_id(args, 1)?)
    };

    // Inserting a node before itself is a no-op
    if ref_id == Some(new_id) {
        return Ok(args[0].clone());
    }

    // Inserting a DocumentFragment moves its children
    let node_ids = insertable_node_ids(&ctx, new_id);

    let mut doc = ctx.doc.borrow_mut();
    let mut mutr = doc.mutate();
    detach_all(&mut mutr, &node_ids);
    match ref_id {
        Some(ref_id) if mutr.node_has_parent(ref_id) => {
            mutr.insert_nodes_before(ref_id, &node_ids);
        }
        _ => mutr.append_children(parent_id, &node_ids),
    }
    Ok(args[0].clone())
}

fn remove_child(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _parent_id = this_node_id(this)?;
    let child_id = arg_node_id(args, 0)?;

    let mut doc = ctx.doc.borrow_mut();
    // Note: the node is detached rather than dropped so that JS wrappers
    // referencing it (or its descendants) remain valid.
    doc.mutate().remove_node(child_id);
    Ok(args[0].clone())
}

fn replace_child(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let _parent_id = this_node_id(this)?;
    let new_id = arg_node_id(args, 0)?;
    let old_id = arg_node_id(args, 1)?;

    if new_id != old_id {
        let mut doc = ctx.doc.borrow_mut();
        let mut mutr = doc.mutate();
        if mutr.node_has_parent(new_id) {
            mutr.remove_node(new_id);
        }
        mutr.insert_nodes_before(old_id, &[new_id]);
        mutr.remove_node(old_id);
    }
    Ok(args[1].clone())
}

fn remove(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let mut doc = ctx.doc.borrow_mut();
    let mut mutr = doc.mutate();
    if mutr.node_has_parent(node_id) {
        mutr.remove_node(node_id);
    }
    Ok(JsValue::undefined())
}

// === DocumentFragment support ===

/// Is the node a DocumentFragment (e.g. a `<template>`'s contents, or created
/// with `document.createDocumentFragment()`)? Fragments are represented as
/// detached elements with the special name `#document-fragment`.
pub(crate) fn is_fragment(doc: &blitz_dom::BaseDocument, node_id: NodeId) -> bool {
    doc.get_node(node_id)
        .and_then(|node| node.element_data())
        .is_some_and(|element| &*element.name.local == "#document-fragment")
}

/// Resolve a node argument for insertion, per the DOM spec's "insert a node"
/// steps: DocumentFragments are expanded into (and their children detached
/// from) their children; other nodes insert as themselves.
fn insertable_node_ids(ctx: &DomCtx, node_id: NodeId) -> Vec<NodeId> {
    let mut doc = ctx.doc.borrow_mut();
    if !is_fragment(&doc, node_id) {
        return vec![node_id];
    }
    let child_ids: Vec<NodeId> = doc
        .get_node(node_id)
        .map(|node| node.children.to_vec())
        .unwrap_or_default();
    let mut mutr = doc.mutate();
    for child_id in &child_ids {
        mutr.remove_node(*child_id);
    }
    child_ids
}

// === ParentNode / ChildNode mixin mutation helpers ===
//
// These accept any number of arguments, each either a node or a string
// (strings are converted to text nodes).

/// Convert the arguments of a ParentNode/ChildNode mutation method into node
/// ids, creating (detached) text nodes for string arguments and expanding
/// DocumentFragments into their children
fn arg_node_ids(ctx: &DomCtx, args: &[JsValue], context: &mut Context) -> JsResult<Vec<NodeId>> {
    let mut node_ids = Vec::with_capacity(args.len());
    for arg in args {
        match node_id_of_value(arg) {
            Some(node_id) => node_ids.extend(insertable_node_ids(ctx, node_id)),
            None => {
                let text = to_rust_string(arg, context)?;
                node_ids.push(ctx.doc.borrow_mut().mutate().create_text_node(&text));
            }
        };
    }
    Ok(node_ids)
}

/// Detach (rather than drop) any already-parented nodes so that JS wrappers
/// referencing them remain valid
fn detach_all(mutr: &mut blitz_dom::DocumentMutator<'_>, node_ids: &[NodeId]) {
    for node_id in node_ids {
        if mutr.node_has_parent(*node_id) {
            mutr.remove_node(*node_id);
        }
    }
}

pub(crate) fn append(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let parent_id = this_node_id(this)?;
    let node_ids = arg_node_ids(&ctx, args, context)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.mutate().append_children(parent_id, &node_ids);
    Ok(JsValue::undefined())
}

pub(crate) fn prepend(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let parent_id = this_node_id(this)?;
    let node_ids = arg_node_ids(&ctx, args, context)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.mutate().prepend_nodes(parent_id, &node_ids);
    Ok(JsValue::undefined())
}

pub(crate) fn replace_children(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let parent_id = this_node_id(this)?;
    let node_ids = arg_node_ids(&ctx, args, context)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.mutate().replace_children(parent_id, &node_ids);
    Ok(JsValue::undefined())
}

fn before(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let anchor_id = this_node_id(this)?;
    let node_ids = arg_node_ids(&ctx, args, context)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.mutate().before_node(anchor_id, &node_ids);
    Ok(JsValue::undefined())
}

fn after(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let anchor_id = this_node_id(this)?;
    let node_ids = arg_node_ids(&ctx, args, context)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.mutate().after_node(anchor_id, &node_ids);
    Ok(JsValue::undefined())
}

fn replace_with(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let anchor_id = this_node_id(this)?;
    let node_ids = arg_node_ids(&ctx, args, context)?;
    let mut doc = ctx.doc.borrow_mut();
    doc.mutate().replace_with_nodes(anchor_id, &node_ids);
    Ok(JsValue::undefined())
}

fn has_child_nodes(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let has_children = ctx
        .doc
        .borrow()
        .get_node(node_id)
        .is_some_and(|node| !node.children.is_empty());
    Ok(JsValue::from(has_children))
}

fn contains(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let Some(mut current) = args.first().and_then(node_id_of_value) else {
        return Ok(JsValue::from(false));
    };

    let doc = ctx.doc.borrow();
    loop {
        if current == node_id {
            return Ok(JsValue::from(true));
        }
        match doc.get_node(current).and_then(|node| node.parent) {
            Some(parent_id) => current = parent_id,
            None => return Ok(JsValue::from(false)),
        }
    }
}

fn clone_node(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let deep = args.first().map(JsValue::to_boolean).unwrap_or(false);

    enum CloneSrc {
        Element(blitz_dom::QualName, Vec<blitz_dom::Attribute>),
        Text(String),
        Other,
    }

    let new_node_id = {
        let mut doc = ctx.doc.borrow_mut();
        if deep {
            doc.mutate().deep_clone_node(node_id)
        } else {
            let src = match doc.get_node(node_id).map(|node| &node.data) {
                Some(NodeData::Element(data)) => {
                    CloneSrc::Element(data.name.clone(), data.attrs().to_vec())
                }
                Some(NodeData::Text(data)) => CloneSrc::Text(data.content.clone()),
                _ => CloneSrc::Other,
            };
            let mut mutr = doc.mutate();
            match src {
                CloneSrc::Element(name, attrs) => mutr.create_element(name, attrs),
                CloneSrc::Text(content) => mutr.create_text_node(&content),
                CloneSrc::Other => mutr.create_comment_node(""),
            }
        }
    };

    Ok(node_wrapper(&ctx, new_node_id, context).into())
}
