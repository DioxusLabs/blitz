//! Reflected attributes on iframe wrappers.

use boa_engine::{Context, JsResult, JsValue, object::JsObject};
use url::Url;

use super::element::{read_attr, write_attr};
use super::{define_accessor, dom_ctx, js_str, this_node_id, to_rust_string};

pub(super) fn init_iframe(wrapper: &JsObject, context: &mut Context) {
    define_accessor(wrapper, "src", Some(get_src), Some(set_src), context);
    macro_rules! reflect {
        ($name:literal, $get:ident, $set:ident) => {
            fn $get(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
                let ctx = dom_ctx(context)?;
                let node_id = this_node_id(this)?;
                Ok(js_str(&read_attr(&ctx, node_id, $name).unwrap_or_default()))
            }
            fn $set(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
                set_attribute($name, this, args, context)
            }
            define_accessor(wrapper, $name, Some($get), Some($set), context);
        };
    }
    reflect!("srcdoc", get_srcdoc, set_srcdoc);
    reflect!("name", get_name, set_name);
    reflect!("width", get_width, set_width);
    reflect!("height", get_height, set_height);
    reflect!("allow", get_allow, set_allow);
}

fn get_src(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let Some(src) = read_attr(&ctx, node_id, "src") else {
        return Ok(js_str(""));
    };
    let base_url = ctx.state.borrow().base_url.clone();
    let resolved = match base_url {
        Some(base) => base.join(&src),
        None => Url::parse(&src),
    };
    Ok(js_str(&resolved.map(|url| url.to_string()).unwrap_or(src)))
}

fn set_src(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    set_attribute("src", this, args, context)
}

fn set_attribute(
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
