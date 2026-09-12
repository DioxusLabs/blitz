//! `HTMLHyperlinkElementUtils`: `href` and the URL-decomposition accessors
//! (`protocol`, `host`, `pathname`, ...) on `<a>` and `<area>` elements,
//! resolved against the document base URL and reflected into the `href`
//! attribute on assignment. `<link>` and `<base>` reflect `href` only.

use blitz_dom::NodeId;
use boa_engine::object::JsObject;
use boa_engine::value::JsValue;
use boa_engine::{Context, JsResult};
use url::Url;

use super::element::{read_attr, write_attr};
use super::{define_accessor, dom_ctx, js_str, this_node_id, to_rust_string};
use crate::state::DomCtx;

pub(crate) fn init_hyperlink_accessors(proto: &JsObject, context: &mut Context) {
    define_accessor(proto, "href", Some(get_href), Some(set_href), context);
    macro_rules! component {
        ($name:literal, $get:ident, $set:ident, $component:expr) => {
            fn $get(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
                component_getter($component, this, context)
            }
            fn $set(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
                component_setter($component, this, args, context)
            }
            define_accessor(proto, $name, Some($get), Some($set), context);
        };
    }
    component!("origin", get_origin, set_origin, Component::Origin);
    component!("protocol", get_protocol, set_protocol, Component::Protocol);
    component!("username", get_username, set_username, Component::Username);
    component!("password", get_password, set_password, Component::Password);
    component!("host", get_host, set_host, Component::Host);
    component!("hostname", get_hostname, set_hostname, Component::Hostname);
    component!("port", get_port, set_port, Component::Port);
    component!("pathname", get_pathname, set_pathname, Component::Pathname);
    component!("search", get_search, set_search, Component::Search);
    component!("hash", get_hash, set_hash, Component::Hash);
}

#[derive(Clone, Copy)]
enum Component {
    Origin,
    Protocol,
    Username,
    Password,
    Host,
    Hostname,
    Port,
    Pathname,
    Search,
    Hash,
}

fn local_name(ctx: &DomCtx, node_id: NodeId) -> Option<String> {
    let doc = ctx.doc.borrow();
    let element = doc.get_node(node_id)?.element_data()?;
    Some(element.name.local.to_string())
}

fn is_hyperlink(ctx: &DomCtx, node_id: NodeId) -> bool {
    matches!(local_name(ctx, node_id).as_deref(), Some("a" | "area"))
}

fn reflects_href(ctx: &DomCtx, node_id: NodeId) -> bool {
    matches!(
        local_name(ctx, node_id).as_deref(),
        Some("a" | "area" | "link" | "base")
    )
}

/// The element's `href` attribute resolved against the document base URL, or
/// `None` if the attribute is missing or does not parse
fn resolved_url(ctx: &DomCtx, node_id: NodeId) -> Option<Url> {
    let href = read_attr(ctx, node_id, "href")?;
    let base_url = ctx.state.borrow().base_url.clone();
    match base_url {
        Some(base) => base.join(&href).ok(),
        None => Url::parse(&href).ok(),
    }
}

fn get_href(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    if !reflects_href(&ctx, node_id) {
        return Ok(JsValue::undefined());
    }
    let Some(href) = read_attr(&ctx, node_id, "href") else {
        return Ok(js_str(""));
    };
    let href = resolved_url(&ctx, node_id)
        .map(|url| url.to_string())
        .unwrap_or(href);
    Ok(js_str(&href))
}

fn set_href(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let value = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    write_attr(&ctx, node_id, "href", &value);
    Ok(JsValue::undefined())
}

fn component_getter(
    component: Component,
    this: &JsValue,
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    if !is_hyperlink(&ctx, node_id) {
        return Ok(JsValue::undefined());
    }
    let Some(url) = resolved_url(&ctx, node_id) else {
        let value = match component {
            Component::Protocol => ":",
            _ => "",
        };
        return Ok(js_str(value));
    };
    let value = match component {
        Component::Origin => url.origin().ascii_serialization(),
        Component::Protocol => format!("{}:", url.scheme()),
        Component::Username => url.username().to_string(),
        Component::Password => url.password().unwrap_or_default().to_string(),
        Component::Host => match (url.host_str(), url.port()) {
            (Some(host), Some(port)) => format!("{host}:{port}"),
            (Some(host), None) => host.to_string(),
            (None, _) => String::new(),
        },
        Component::Hostname => url.host_str().unwrap_or_default().to_string(),
        Component::Port => url.port().map(|p| p.to_string()).unwrap_or_default(),
        Component::Pathname => url.path().to_string(),
        Component::Search => match url.query() {
            Some(query) if !query.is_empty() => format!("?{query}"),
            _ => String::new(),
        },
        Component::Hash => match url.fragment() {
            Some(fragment) if !fragment.is_empty() => format!("#{fragment}"),
            _ => String::new(),
        },
    };
    Ok(js_str(&value))
}

fn component_setter(
    component: Component,
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let ctx = dom_ctx(context)?;
    let node_id = this_node_id(this)?;
    let value = to_rust_string(args.first().unwrap_or(&JsValue::undefined()), context)?;
    if !is_hyperlink(&ctx, node_id) {
        return Ok(JsValue::undefined());
    }
    let Some(mut url) = resolved_url(&ctx, node_id) else {
        return Ok(JsValue::undefined());
    };
    let value = value.as_str();
    let ok = match component {
        Component::Origin => true,
        Component::Protocol => url
            .set_scheme(value.strip_suffix(':').unwrap_or(value))
            .is_ok(),
        Component::Username => url.set_username(value).is_ok(),
        Component::Password => url
            .set_password((!value.is_empty()).then_some(value))
            .is_ok(),
        Component::Host => match value.rsplit_once(':') {
            Some((host, port)) if !host.is_empty() && !host.ends_with(']') => {
                url.set_host(Some(host)).is_ok() && url.set_port(port.parse().ok()).is_ok()
            }
            _ => url.set_host(Some(value)).is_ok(),
        },
        Component::Hostname => url.set_host(Some(value)).is_ok(),
        Component::Port => url
            .set_port(if value.is_empty() {
                None
            } else {
                value.parse().ok()
            })
            .is_ok(),
        Component::Pathname => {
            if !url.cannot_be_a_base() {
                url.set_path(value);
            }
            true
        }
        Component::Search => {
            let query = value.strip_prefix('?').unwrap_or(value);
            url.set_query((!query.is_empty()).then_some(query));
            true
        }
        Component::Hash => {
            let fragment = value.strip_prefix('#').unwrap_or(value);
            url.set_fragment((!fragment.is_empty()).then_some(fragment));
            true
        }
    };
    if ok {
        write_attr(&ctx, node_id, "href", url.as_str());
    }
    Ok(JsValue::undefined())
}
