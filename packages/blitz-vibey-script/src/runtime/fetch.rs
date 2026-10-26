//! The native resource bridge used by the JavaScript fetch implementation.

use boa_engine::{Context, JsNativeError, JsResult, JsString, JsValue};
use url::Url;

use crate::dom::dom_ctx;
use crate::fetch::{FetchError, document_may_load};

/// `__blitz_fetch_sync(url)`: resolve `url` against the document base URL and
/// fetch it via the document's [`ScriptFetcher`](crate::fetch::ScriptFetcher) if the
/// document may load it ([`document_may_load`]). Returns `[status, url, text]`:
/// a missing resource yields a 404 (like an HTTP server would), any other
/// failure throws a `TypeError` (a network error, in `fetch()` terms).
pub(super) fn fetch_sync(
    _: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    use boa_engine::object::builtins::JsArray;

    let ctx = dom_ctx(context)?;
    let input = args
        .first()
        .unwrap_or(&JsValue::undefined())
        .to_string(context)?
        .to_std_string_lossy();

    let (base_url, fetcher) = {
        let state = ctx.state.borrow();
        (state.base_url.clone(), state.fetcher.clone())
    };
    let url = match &base_url {
        Some(base) => base.join(&input),
        None => Url::parse(&input),
    }
    .map_err(|_| JsNativeError::typ().with_message(format!("Failed to parse URL from {input}")))?;
    let fetcher =
        fetcher.ok_or_else(|| JsNativeError::typ().with_message("fetch is unavailable"))?;

    let result = if document_may_load(base_url.as_ref(), &url) {
        fetcher.borrow().fetch(&url)
    } else {
        Err(FetchError::Forbidden(url.clone()))
    };
    let (status, text) = match result {
        Ok(text) => (200, text),
        Err(FetchError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            (404, String::new())
        }
        Err(error) => {
            return Err(JsNativeError::typ()
                .with_message(format!("Failed to fetch {url}: {error}"))
                .into());
        }
    };

    Ok(JsArray::from_iter(
        [
            JsValue::from(status),
            JsString::from(url.as_str()).into(),
            JsString::from(text).into(),
        ],
        context,
    )
    .into())
}
