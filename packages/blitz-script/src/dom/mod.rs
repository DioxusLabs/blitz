//! JavaScript DOM API bindings backed by `blitz-dom`.
//!
//! The bindings are implemented as prototype objects built with Boa's native object
//! APIs. Each DOM node is represented by a single cached JS wrapper object (see
//! [`node_wrapper`]) whose native data is a [`NodeRef`] holding the `blitz-dom`
//! node id. Native functions look the document up via the [`DomCtx`] stored as
//! host-defined data on the Boa [`Context`].

pub(crate) mod document;
pub(crate) mod element;
pub(crate) mod event;
pub(crate) mod node;
pub(crate) mod style;

use blitz_dom::node::NodeData;
use blitz_dom::{LocalName, Namespace, NodeId, QualName};
use boa_engine::object::{FunctionObjectBuilder, JsObject};
use boa_engine::property::{PropertyDescriptor, PropertyKey};
use boa_engine::value::JsValue;
use boa_engine::{
    Context, Finalize, JsData, JsNativeError, JsResult, JsString, NativeFunction, Trace,
};

use crate::state::{DomCtx, DomProtos};

/// Native data attached to DOM node wrapper objects
#[derive(Trace, Finalize, JsData)]
pub(crate) struct NodeRef {
    #[unsafe_ignore_trace]
    pub node_id: NodeId,
}

/// Fetch the [`DomCtx`] from the Boa context's host-defined data
pub(crate) fn dom_ctx(context: &mut Context) -> JsResult<DomCtx> {
    context.get_data::<DomCtx>().cloned().ok_or_else(|| {
        JsNativeError::typ()
            .with_message("DOM context missing")
            .into()
    })
}

/// Extract the node id from a JS DOM node wrapper
pub(crate) fn node_id_of_value(value: &JsValue) -> Option<NodeId> {
    value.as_object().and_then(|obj| {
        obj.downcast_ref::<NodeRef>()
            .map(|node_ref| node_ref.node_id)
    })
}

/// Extract the node id from the `this` value of a native function
pub(crate) fn this_node_id(this: &JsValue) -> JsResult<NodeId> {
    node_id_of_value(this).ok_or_else(|| {
        JsNativeError::typ()
            .with_message("`this` is not a DOM node")
            .into()
    })
}

/// Get (or create) the unique JS wrapper object for a DOM node.
///
/// Wrappers are cached in [`RuntimeState::node_wrappers`](crate::state::RuntimeState::node_wrappers)
/// so that object identity (`===`) and expando properties behave as scripts expect.
pub(crate) fn node_wrapper(ctx: &DomCtx, node_id: NodeId, _context: &mut Context) -> JsObject {
    if let Some(wrapper) = ctx.state.borrow().node_wrappers.get(&node_id) {
        return wrapper.clone();
    }

    let proto = {
        let doc = ctx.doc.borrow();
        let state = ctx.state.borrow();
        let protos = state.protos();
        match doc.get_node(node_id).map(|node| &node.data) {
            Some(NodeData::Document(_)) => protos.document.clone(),
            Some(NodeData::Element(_)) | Some(NodeData::AnonymousBlock(_)) => {
                protos.element.clone()
            }
            Some(NodeData::Text(_)) | Some(NodeData::Comment { .. }) => protos.character_data.clone(),
            None => protos.node.clone(),
        }
    };

    let wrapper = JsObject::from_proto_and_data(Some(proto), NodeRef { node_id });
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

type NativeFnPtr = fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>;

/// Define a method on a prototype object
pub(crate) fn define_method(
    obj: &JsObject,
    name: &str,
    length: usize,
    body: NativeFnPtr,
    context: &mut Context,
) {
    let function = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(body))
        .name(JsString::from(name))
        .length(length)
        .build();
    obj.define_property_or_throw(
        PropertyKey::from(JsString::from(name)),
        PropertyDescriptor::builder()
            .value(function)
            .writable(true)
            .enumerable(false)
            .configurable(true)
            .build(),
        context,
    )
    .expect("failed to define DOM method");
}

/// Define an accessor (getter/setter pair) on a prototype object
pub(crate) fn define_accessor(
    obj: &JsObject,
    name: &str,
    getter: Option<NativeFnPtr>,
    setter: Option<NativeFnPtr>,
    context: &mut Context,
) {
    let getter = getter.map(|g| {
        FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(g))
            .name(JsString::from(format!("get {name}")))
            .length(0)
            .build()
    });
    let setter = setter.map(|s| {
        FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(s))
            .name(JsString::from(format!("set {name}")))
            .length(1)
            .build()
    });
    let mut builder = PropertyDescriptor::builder()
        .enumerable(false)
        .configurable(true);
    if let Some(getter) = getter {
        builder = builder.get(getter);
    }
    if let Some(setter) = setter {
        builder = builder.set(setter);
    }
    obj.define_property_or_throw(
        PropertyKey::from(JsString::from(name)),
        builder.build(),
        context,
    )
    .expect("failed to define DOM accessor");
}

/// Define a plain data property
pub(crate) fn define_value(obj: &JsObject, name: &str, value: JsValue, context: &mut Context) {
    obj.define_property_or_throw(
        PropertyKey::from(JsString::from(name)),
        PropertyDescriptor::builder()
            .value(value)
            .writable(true)
            .enumerable(false)
            .configurable(true)
            .build(),
        context,
    )
    .expect("failed to define DOM property");
}

/// Event types for which `on<event>` IDL-style properties are defined on the
/// node prototype. Frameworks (e.g. Preact) use `'onclick' in dom` checks to
/// infer the correct casing of event names, so these need to be present.
pub(crate) const ON_EVENT_TYPES: &[&str] = &[
    "click",
    "dblclick",
    "contextmenu",
    "mousedown",
    "mouseup",
    "mousemove",
    "mouseenter",
    "mouseleave",
    "mouseover",
    "mouseout",
    "pointerdown",
    "pointerup",
    "pointermove",
    "pointercancel",
    "pointerenter",
    "pointerleave",
    "pointerover",
    "pointerout",
    "touchstart",
    "touchmove",
    "touchend",
    "touchcancel",
    "keydown",
    "keyup",
    "keypress",
    "input",
    "change",
    "focus",
    "blur",
    "focusin",
    "focusout",
    "submit",
    "scroll",
    "wheel",
    "load",
];

/// Initialise the DOM prototype objects and store them in the runtime state
pub(crate) fn init_protos(ctx: &DomCtx, context: &mut Context) {
    let object_proto = context.intrinsics().constructors().object().prototype();

    let node_proto = JsObject::with_object_proto(context.intrinsics());
    node::init_node_proto(&node_proto, context);

    // `on<event>` IDL-style properties (default null)
    for event_type in ON_EVENT_TYPES {
        define_value(
            &node_proto,
            &format!("on{event_type}"),
            JsValue::null(),
            context,
        );
    }

    let element_proto = JsObject::with_object_proto(context.intrinsics());
    element_proto.set_prototype(Some(node_proto.clone()));
    element::init_element_proto(&element_proto, context);

    let character_data_proto = JsObject::with_object_proto(context.intrinsics());
    character_data_proto.set_prototype(Some(node_proto.clone()));
    node::init_character_data_proto(&character_data_proto, context);

    let document_proto = JsObject::with_object_proto(context.intrinsics());
    document_proto.set_prototype(Some(node_proto.clone()));
    document::init_document_proto(&document_proto, context);

    let event_proto = JsObject::with_object_proto(context.intrinsics());
    event::init_event_proto(&event_proto, context);

    let style_proto = JsObject::with_object_proto(context.intrinsics());
    style::init_style_proto(&style_proto, context);

    let _ = object_proto;

    ctx.state.borrow_mut().protos = Some(DomProtos {
        node: node_proto,
        element: element_proto,
        character_data: character_data_proto,
        document: document_proto,
        event: event_proto,
        style: style_proto,
    });
}
