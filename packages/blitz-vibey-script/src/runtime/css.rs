//! Native CSS namespace, transform parsing, and computed-style bindings.

use boa_engine::object::JsObject;
use boa_engine::{Context, JsNativeError, JsResult, JsValue, js_string};

use crate::dom::{NodeRef, dom_ctx, node_id_of_value, to_rust_string, wrap_style_object};

pub(super) fn css_property_supported(
    _: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let name = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    Ok(JsValue::from(blitz_dom::css_property_is_supported(&name)))
}

/// `__blitz_parse_transform(string)`: parse a CSS `<transform-list>` into a
/// 4x4 matrix. Returns `[elements, is2D]` (16 column-major elements), or
/// `null` if the string is not a valid transform list.
pub(super) fn parse_transform(
    _: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let value = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let Some((elements, is_2d)) = blitz_dom::parse_transform_matrix(&value) else {
        return Ok(JsValue::null());
    };
    let elements = boa_engine::object::builtins::JsArray::from_iter(
        elements.into_iter().map(JsValue::from),
        context,
    );
    Ok(boa_engine::object::builtins::JsArray::from_iter(
        [elements.into(), JsValue::from(is_2d)],
        context,
    )
    .into())
}

/// `CSS.supports()`: the two-argument form checks a property/value declaration,
/// the one-argument form evaluates a `@supports` condition
pub(super) fn css_supports(
    _: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let supported = if args.len() >= 2 {
        let property = to_rust_string(&args[0], context)?;
        let value = to_rust_string(&args[1], context)?;
        ctx.doc.borrow().css_declaration_is_valid(&property, &value)
    } else {
        let condition = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
        ctx.doc.borrow().css_supports_condition(&condition)
    };
    Ok(JsValue::from(supported))
}

/// `CSS.registerProperty({ name, syntax = "*", inherits, initialValue })`
/// <https://drafts.css-houdini.org/css-properties-values-api-1/#the-registerproperty-function>
pub(super) fn css_register_property(
    _: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    use blitz_dom::RegisterCustomPropertyResult as Result_;

    let ctx = dom_ctx(context)?;
    let Some(descriptor) = args.first().and_then(JsValue::as_object) else {
        return Err(JsNativeError::typ()
            .with_message("CSS.registerProperty: argument must be a PropertyDefinition dictionary")
            .into());
    };
    let get = |key: &str, context: &mut Context| -> JsResult<Option<String>> {
        let value = descriptor.get(js_string!(key), context)?;
        if value.is_undefined() {
            return Ok(None);
        }
        Ok(Some(to_rust_string(&value, context)?))
    };
    let Some(name) = get("name", context)? else {
        return Err(JsNativeError::typ()
            .with_message("CSS.registerProperty: 'name' is required")
            .into());
    };
    let inherits = descriptor.get(js_string!("inherits"), context)?;
    if inherits.is_undefined() {
        return Err(JsNativeError::typ()
            .with_message("CSS.registerProperty: 'inherits' is required")
            .into());
    }
    let inherits = inherits.to_boolean();
    let syntax = get("syntax", context)?.unwrap_or_else(|| "*".to_string());
    let initial_value = get("initialValue", context)?;

    let result = ctx.doc.borrow_mut().register_custom_property(
        &name,
        &syntax,
        inherits,
        initial_value.as_deref(),
    );
    let error = match result {
        Result_::SuccessfullyRegistered => return Ok(JsValue::undefined()),
        Result_::InvalidName => JsNativeError::syntax().with_message(format!(
            "CSS.registerProperty: '{name}' is not a valid custom property name"
        )),
        Result_::AlreadyRegistered => JsNativeError::error().with_message(format!(
            "CSS.registerProperty: '{name}' is already registered"
        )),
        Result_::InvalidSyntax => JsNativeError::syntax()
            .with_message(format!("CSS.registerProperty: invalid syntax '{syntax}'")),
        Result_::NoInitialValue => JsNativeError::syntax().with_message(
            "CSS.registerProperty: 'initialValue' is required for non-universal syntax",
        ),
        Result_::InvalidInitialValue | Result_::InitialValueNotComputationallyIndependent => {
            JsNativeError::syntax().with_message("CSS.registerProperty: invalid 'initialValue'")
        }
    };
    Err(error.into())
}

pub(super) fn get_computed_style(
    _: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let Some(node_id) = args.first().and_then(node_id_of_value) else {
        return Err(boa_engine::JsNativeError::typ()
            .with_message("getComputedStyle: argument is not an Element")
            .into());
    };
    let proto = ctx.state.borrow().protos().computed_style.clone();
    let obj = JsObject::from_proto_and_data(Some(proto), NodeRef { node_id });
    Ok(wrap_style_object(obj, context))
}
