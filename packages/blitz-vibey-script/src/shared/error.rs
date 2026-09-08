// ── Error macro ──────────────────────────────────────────────────────
#![allow(unused)]

/// Shorthand for constructing a `JsError` from a native error kind + message.
///
/// ```ignore
/// native_error!(typ, "not a Node")
/// native_error!(reference, "node {id} not found", id)
/// ```
macro_rules! native_error {
    ($ctor:ident, $fmt:literal $(, $arg:expr)* $(,)?) => {
        boa_engine::JsError::from(boa_engine::JsNativeError::$ctor().with_message(format!($fmt, $($arg),*)))
    };
    ($ctor:ident($errors: expr), $fmt:literal $(, $arg:expr)* $(,)?) => {
        boa_engine::JsError::from(boa_engine::JsNativeError::$ctor($errors).with_message(format!($fmt, $($arg),*)), None)
    };
}
pub(crate) use native_error;

// ── Downcast helpers ─────────────────────────────────────────────────

/// Construct a "node not found" ReferenceError.
macro_rules! err_node {
    ($id:expr) => {
        || native_error!(reference, "node {:?} not found", $id)
    };
}
pub(crate) use err_node;
