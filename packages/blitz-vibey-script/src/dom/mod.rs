//! JavaScript DOM API bindings backed by `blitz-dom`.
//!
//! The bindings use the `Extended<T>` layer scheme (see `crate::shared::extends`):
//! every DOM interface is an `ExtendLayer`, and each node wrapper is built from
//! its layer chain via `from_chain!`. Each DOM node is represented by a single
//! cached JS wrapper object (see `node_wrapper`) so that object identity (`===`)
//! and expando properties behave as scripts expect. Native functions look the
//! document up via the `DomCtx` stored as host-defined data on the Boa
//! `Context`.

pub(crate) mod body;
pub(crate) mod character_data;
pub(crate) mod comment;
pub(crate) mod document;
pub(crate) mod element;
pub(crate) mod node;
pub(crate) mod style;
pub(crate) mod text;

use blitz_dom::node::NodeData;
use blitz_dom::{LocalName, Namespace, NodeId, QualName};
use boa_engine::object::JsObject;
use boa_engine::value::JsValue;
use boa_engine::{Context, JsNativeError, JsResult, JsString};

use crate::shared::{as_object, from_chain, layer_chain, with_own};
use crate::state::DomCtx;

use node::NodeLayer;

/// Fetch the [`DomCtx`] from the Boa context's host-defined data
pub(crate) fn dom_ctx(context: &mut Context) -> JsResult<DomCtx> {
    context.get_data::<DomCtx>().cloned().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("DOM context missing")
            .into()
    })
}

/// Extract the node id from a DOM node wrapper's `Node` own block
pub(crate) fn node_id_of_value(value: &JsValue) -> Option<NodeId> {
    let obj = value.as_object()?;
    with_own::<NodeLayer, _>(&obj, |node| node.node_id).ok()
}

/// Extract the node id from the `this` value of a native function
pub(crate) fn this_node_id(this: &JsValue) -> JsResult<NodeId> {
    let obj = as_object!(this, typ, "`this` is not a DOM node")?;
    with_own::<NodeLayer, _>(&obj, |node| node.node_id)
}

/// Get (or create) the unique JS wrapper object for a DOM node.
///
/// Wrappers are cached in the runtime state (`RuntimeState::node_wrappers`) so
/// that object identity (`===`) and expando properties behave as scripts expect.
pub(crate) fn node_wrapper(ctx: &DomCtx, node_id: NodeId, context: &mut Context) -> JsObject {
    if let Some(wrapper) = ctx.state.borrow().node_wrappers.get(node_id) {
        return wrapper;
    }

    // Classify the node first: the document borrow must not be held across
    // wrapper construction.
    enum WrapperKind {
        Document,
        Element,
        Text,
        Comment,
        Node,
    }
    let (kind, is_body) = {
        let doc = ctx.doc.borrow();
        let node_data = doc.get_node(node_id).map(|node| &node.data);
        let kind = match node_data {
            Some(NodeData::Document(_)) => WrapperKind::Document,
            Some(NodeData::Element(_)) | Some(NodeData::AnonymousBlock(_)) => WrapperKind::Element,
            Some(NodeData::Text(_)) => WrapperKind::Text,
            Some(NodeData::Comment { .. }) => WrapperKind::Comment,
            None => WrapperKind::Node,
        };
        // `<body>` / `<frameset>` wrappers sit one layer higher: the
        // window-reflecting event handler accessors defined by `BodyLayer`.
        let is_body = matches!(
            node_data,
            Some(NodeData::Element(element))
                if element.name.local == blitz_dom::local_name!("body")
                    || element.name.local == blitz_dom::local_name!("frameset")
        );
        (kind, is_body)
    };

    let base_later = layer_chain!(
        crate::events::EventTargetLayer {
            listeners: Default::default(),
        },
        NodeLayer { node_id },
    );

    let wrapper = match kind {
        WrapperKind::Document => from_chain!(
            (document::Document, context),
            ..base_later,
            document::DocumentLayer,
        )
        .expect("failed to build Document wrapper"),
        WrapperKind::Element => {
            if is_body {
                from_chain!(
                    (body::Body, context),
                    ..base_later,
                    element::ElementLayer,
                    body::BodyLayer,
                )
                .expect("failed to build HTMLBodyElement wrapper")
            } else {
                from_chain!(
                    (element::Element, context),
                    ..base_later,
                    element::ElementLayer,
                )
                .expect("failed to build Element wrapper")
            }
        }
        WrapperKind::Text => from_chain!(
            (text::Text, context),
            ..base_later,
            character_data::CharacterDataLayer,
            text::TextLayer,
        )
        .expect("failed to build Text wrapper"),
        WrapperKind::Comment => from_chain!(
            (comment::Comment, context),
            ..base_later,
            character_data::CharacterDataLayer,
            comment::CommentLayer,
        )
        .expect("failed to build Comment wrapper"),
        WrapperKind::Node => {
            from_chain!((node::Node, context), ..base_later,).expect("failed to build Node wrapper")
        }
    };

    ctx.state
        .borrow_mut()
        .node_wrappers
        .insert(node_id, wrapper.clone());
    wrapper
}

/// Convert an optional node id to a JS value (wrapper object or `null`)
pub(crate) fn node_or_null(
    ctx: &DomCtx,
    node_id: Option<NodeId>,
    context: &mut Context,
) -> JsValue {
    match node_id {
        Some(node_id) => node_wrapper(ctx, node_id, context).into(),
        None => JsValue::null(),
    }
}

/// Construct an HTML `QualName` from a string
pub(crate) fn qual_name(local: &str) -> QualName {
    QualName::new(None, markup5ever::ns!(html), LocalName::from(local))
}

/// Construct a `QualName` in the given namespace from a string
pub(crate) fn qual_name_ns(local: &str, ns: &str) -> QualName {
    QualName::new(None, Namespace::from(ns), LocalName::from(local))
}

pub(crate) fn js_str(s: &str) -> JsValue {
    JsString::from(s).into()
}

/// Convert a JS value to a Rust `String` (via ECMAScript `ToString`)
pub(crate) fn to_rust_string(value: &JsValue, context: &mut Context) -> JsResult<String> {
    Ok(value.to_string(context)?.to_std_string_lossy())
}

/// Register all DOM classes and wire up their prototype chains.
pub(crate) fn register_dom_classes(context: &mut Context) -> JsResult<()> {
    // Event-target side first: `Node` links to `EventTarget`
    crate::events::register(context)?;
    node::register(context)?;
    character_data::register(context)?;
    text::register(context)?;
    comment::register(context)?;
    element::register(context)?;
    body::register(context)?;
    document::register(context)?;
    style::register(context)?;

    Ok(())
}

/// Collect the descendants of `root_id` (excluding `root_id` itself, in document
/// order) whose element data matches `filter`.
pub(crate) fn collect_matching_descendants(
    doc: &blitz_dom::BaseDocument,
    root_id: NodeId,
    filter: impl Fn(&blitz_dom::ElementData) -> bool,
) -> Vec<NodeId> {
    let mut matches = Vec::new();
    let mut stack: Vec<NodeId> = doc
        .get_node(root_id)
        .map(|root| root.children.iter().rev().copied().collect())
        .unwrap_or_default();
    while let Some(node_id) = stack.pop() {
        let Some(node) = doc.get_node(node_id) else {
            continue;
        };
        if let Some(element) = node.element_data() {
            if filter(element) {
                matches.push(node_id);
            }
        }
        stack.extend(node.children.iter().rev().copied());
    }
    matches
}

/// `getElementsByClassName` filter: the element's `class` attribute must contain
/// every (whitespace-separated) class in `class_names`
pub(crate) fn matches_class_names(element: &blitz_dom::ElementData, class_names: &[&str]) -> bool {
    let class_attr = element.attr(blitz_dom::local_name!("class")).unwrap_or("");
    class_names
        .iter()
        .all(|name| class_attr.split_whitespace().any(|class| class == *name))
}

/// Construct a `DOMException` error (e.g. name = `"SyntaxError"`) as an instance
/// of the global `DOMException` interface defined in the runtime bootstrap, so
/// that `instanceof`/`constructor` checks (as performed by testharness.js'
/// `assert_throws_dom`) pass. Falls back to a native `TypeError` if the global
/// is missing.
pub(crate) fn dom_exception(
    context: &mut Context,
    name: &str,
    message: &str,
) -> boa_engine::JsError {
    let constructor = context
        .global_object()
        .get(boa_engine::js_string!("DOMException"), context)
        .ok()
        .and_then(|value| value.as_object())
        .filter(|obj| obj.is_constructor());
    if let Some(constructor) = constructor {
        if let Ok(exception) =
            constructor.construct(&[js_str(message), js_str(name)], None, context)
        {
            return boa_engine::JsError::from_opaque(exception.into());
        }
    }
    JsNativeError::typ()
        .with_message(format!("{name}: {message}"))
        .into()
}

/// Wrap a native `CSSStyleDeclaration` object in the JS `Proxy` (defined by the
/// runtime bootstrap script) which maps camelCase property access
/// (e.g. `style.gridTemplateColumns`) to `getPropertyValue`/`setProperty` calls.
pub(crate) fn wrap_style_object(obj: JsObject, context: &mut Context) -> JsValue {
    let wrapper = context
        .global_object()
        .get(boa_engine::js_string!("__blitz_wrap_style"), context)
        .ok()
        .and_then(|value| value.as_object())
        .filter(|obj| obj.is_callable());
    if let Some(wrapper) = wrapper {
        if let Ok(wrapped) = wrapper.call(&JsValue::undefined(), &[obj.clone().into()], context) {
            return wrapped;
        }
    }
    obj.into()
}
