//! `CSSStyleDeclaration` bindings (`element.style` and `getComputedStyle`).

use blitz_dom::NodeId;
use boa_engine::class::ClassBuilder;
use boa_engine::property::Attribute;
use boa_engine::{Context, Finalize, JsData, JsResult, JsValue, Trace};

use crate::shared::{
    Constructed, ExtendLayer, Extended, RootLayer, Super, instance_accessor, instance_getter,
    instance_method, js_fn_ptr, native_error, native_fn_ptr, with_own,
};
use crate::state::DomCtx;

use super::{dom_ctx, js_str, to_rust_string};
use super::element::attr_name;

/// `CSSStyleDeclaration` own block: the styled node.
#[derive(Debug, Default, Clone, Trace, Finalize, JsData)]
pub(crate) struct StyleLayer {
    #[unsafe_ignore_trace]
    pub node_id: NodeId,
}

/// `ComputedStyle` own block: the inspected node.
#[derive(Debug, Default, Clone, Trace, Finalize, JsData)]
pub(crate) struct ComputedStyleLayer {
    #[unsafe_ignore_trace]
    pub node_id: NodeId,
}

pub(crate) type CSSStyleDeclaration = Extended<StyleLayer>;
pub(crate) type ComputedStyle = Extended<ComputedStyleLayer>;

impl ExtendLayer for StyleLayer {
    type Parent = RootLayer;
    const CLASS_NAME: &'static str = "CSSStyleDeclaration";

    fn build(
        _args: &[JsValue],
        _ctx: &mut Context,
        _sup: Super<'_, Self::Parent>,
    ) -> JsResult<Constructed<Self>> {
        Err(native_error!(typ, "Illegal constructor"))
    }

    fn define_members(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        let realm = class.context().realm().clone();
        let attr = Attribute::CONFIGURABLE | Attribute::NON_ENUMERABLE;

        instance_accessor!(
            class,
            "cssText",
            js_fn_ptr!(get_css_text, &realm),
            js_fn_ptr!(set_css_text, &realm),
            attr
        );
        instance_method!(class, "setProperty", 2, native_fn_ptr!(set_property));
        instance_method!(class, "removeProperty", 1, native_fn_ptr!(remove_property));
        instance_method!(class, "getPropertyValue", 1, native_fn_ptr!(get_property_value));

        Ok(())
    }
}

impl ExtendLayer for ComputedStyleLayer {
    type Parent = RootLayer;
    const CLASS_NAME: &'static str = "ComputedStyle";

    fn build(
        _args: &[JsValue],
        _ctx: &mut Context,
        _sup: Super<'_, Self::Parent>,
    ) -> JsResult<Constructed<Self>> {
        Err(native_error!(typ, "Illegal constructor"))
    }

    fn define_members(class: &mut ClassBuilder<'_>) -> JsResult<()> {
        let realm = class.context().realm().clone();
        let attr = Attribute::CONFIGURABLE | Attribute::NON_ENUMERABLE;

        // Read-only `CSSStyleDeclaration`s: property reads return *resolved*
        // values, and mutation methods are no-ops.
        instance_getter!(class, "cssText", js_fn_ptr!(get_empty_string, &realm), attr);
        instance_method!(class, "setProperty", 2, native_fn_ptr!(noop));
        instance_method!(class, "removeProperty", 1, native_fn_ptr!(noop));
        instance_method!(
            class,
            "getPropertyValue",
            1,
            native_fn_ptr!(get_resolved_property_value)
        );

        Ok(())
    }
}

/// Register the style classes (root classes: no prototype links).
pub(crate) fn register(context: &mut Context) -> JsResult<()> {
    context.register_global_class::<CSSStyleDeclaration>()?;
    context.register_global_class::<ComputedStyle>()?;
    Ok(())
}

fn this_style_node_id(this: &JsValue) -> JsResult<NodeId> {
    let obj = this
        .as_object()
        .ok_or_else(|| native_error!(typ, "`this` is not a style object"))?;
    with_own::<StyleLayer, _>(&obj, |style| style.node_id)
}

fn this_computed_style_node_id(this: &JsValue) -> JsResult<NodeId> {
    let obj = this
        .as_object()
        .ok_or_else(|| native_error!(typ, "`this` is not a style object"))?;
    with_own::<ComputedStyleLayer, _>(&obj, |style| style.node_id)
}

fn get_empty_string(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(js_str(""))
}

fn noop(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::undefined())
}

fn get_resolved_property_value(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_computed_style_node_id(this)?;
    let name = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?
        .trim()
        .to_ascii_lowercase();

    let mut doc = ctx.doc.borrow_mut();
    // Resolved values of layout-dependent properties are used values, so make
    // sure style and layout are up to date before reading.
    doc.resolve(0.0);
    let value = doc.resolved_style_value(node_id, &name);
    Ok(js_str(&value))
}

/// Read the node's `style` attribute (empty string if unset)
fn read_style_attr(ctx: &DomCtx, node_id: NodeId) -> String {
    ctx.doc
        .borrow()
        .get_node(node_id)
        .and_then(|node| node.attr(blitz_dom::local_name!("style")))
        .unwrap_or_default()
        .to_string()
}

fn write_style_attr(ctx: &DomCtx, node_id: NodeId, style_attr: &str) {
    let mut doc = ctx.doc.borrow_mut();
    doc.mutate()
        .set_attribute(node_id, attr_name("style"), style_attr);
}

fn get_css_text(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_style_node_id(this)?;
    let style_attr = read_style_attr(&ctx, node_id);
    let css = ctx.doc.borrow().style_attr_serialize(&style_attr);
    Ok(js_str(&css))
}

fn set_css_text(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_style_node_id(this)?;
    let css = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    // Parse and re-serialize so that the stored attribute is canonical and
    // invalid declarations are dropped
    let css = ctx.doc.borrow().style_attr_serialize(&css);
    write_style_attr(&ctx, node_id, &css);
    Ok(JsValue::undefined())
}

fn set_property(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_style_node_id(this)?;
    let name = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let value = to_rust_string(args.get(1).unwrap_or(&JsValue::undefined()), context)?;
    let priority = to_rust_string(args.get(2).unwrap_or(&JsValue::undefined()), context)?;
    let important = priority.eq_ignore_ascii_case("important");

    let style_attr = read_style_attr(&ctx, node_id);
    let new_style_attr =
        ctx.doc
            .borrow()
            .style_attr_set_property(&style_attr, &name, &value, important);
    // `None` means the declaration was invalid: ignore it (per CSSOM)
    if let Some(new_style_attr) = new_style_attr {
        write_style_attr(&ctx, node_id, &new_style_attr);
    }
    Ok(JsValue::undefined())
}

fn remove_property(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_style_node_id(this)?;
    let name = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;

    let style_attr = read_style_attr(&ctx, node_id);
    let removed = ctx
        .doc
        .borrow()
        .style_attr_remove_property(&style_attr, &name);
    let Some((new_style_attr, removed_value)) = removed else {
        return Ok(js_str(""));
    };
    write_style_attr(&ctx, node_id, &new_style_attr);
    Ok(js_str(&removed_value))
}

fn get_property_value(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_style_node_id(this)?;
    let name = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;

    let style_attr = read_style_attr(&ctx, node_id);
    let value = ctx.doc.borrow().style_attr_get_property(&style_attr, &name);
    Ok(js_str(&value))
}
