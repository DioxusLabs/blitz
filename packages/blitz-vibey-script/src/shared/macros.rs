#![allow(unused)]

macro_rules! instance_property {
    ($for: expr, $key:expr, $value:expr, $attr:expr) => {
        $for.property(boa_engine::js_string!($key), $value, $attr)
    };
}

macro_rules! instance_getter {
    ($for: expr, $key:expr, $getter:expr, $attr:expr) => {
        $for.accessor(boa_engine::js_string!($key), Some($getter), None, $attr)
    };
}

macro_rules! instance_setter {
    ($for: expr, $key:expr, $setter:expr, $attr:expr) => {
        $for.accessor(boa_engine::js_string!($key), None, Some($setter), $attr)
    };
}

macro_rules! instance_accessor {
    ($for: expr, $key:expr, $getter:expr, $setter:expr, $attr:expr) => {
        $for.accessor(
            boa_engine::js_string!($key),
            Some($getter),
            Some($setter),
            $attr,
        )
    };
}

macro_rules! instance_method {
    ($for: expr, $key:expr, $len: literal, $fn:expr) => {
        $for.method(boa_engine::js_string!($key), $len, $fn)
    };
}

macro_rules! static_property {
    ($for: expr, $key:expr, $value:expr, $attr:expr) => {
        $for.static_property(boa_engine::js_string!($key), $value, $attr)
    };
}

macro_rules! static_getter {
    ($for: expr, $key:expr, $getter:expr, $attr:expr) => {
        $for.static_accessor(boa_engine::js_string!($key), Some($getter), None, $attr)
    };
}

macro_rules! static_setter {
    ($for: expr, $key:expr, $setter:expr, $attr:expr) => {
        $for.static_accessor(boa_engine::js_string!($key), None, Some($setter), $attr)
    };
}

macro_rules! static_accessor {
    ($for: expr, $key:expr, $getter:expr, $setter:expr, $attr:expr) => {
        $for.static_accessor(
            boa_engine::js_string!($key),
            Some($getter),
            Some($setter),
            $attr,
        )
    };
}

macro_rules! static_method {
    ($for: expr, $key:expr, $len: literal, $fn:expr) => {
        $for.static_method(boa_engine::js_string!($key), $len, $fn)
    };
}

/// Assemble a `LayerChain` (parent layers listed first, own layer last —
/// the `super` sequence).
///
/// The first expr is the deepest parent, the last expr is the outermost own
/// layer; types come from the receiving position or local inference.
///
/// `layer_chain!(A::new(x), B { y })` expands to successive shadowing:
/// ```ignore
/// let acc = ();
/// let acc = LayerChain { own: A::new(x), parent: acc };
/// let acc = LayerChain { own: B { y }, parent: acc };
/// acc
/// ```
///
/// Prefix with `..` to start from an existing chain (the deepest parent):
/// `layer_chain!(.. base_chain, C { z }, D)`
macro_rules! layer_chain {
    // Start from an existing chain
    (.. $base:expr$(, $own:expr)* $(,)?) => {{
        let __layer_chain_acc = $base;
        $( let __layer_chain_acc = $crate::shared::LayerChain { own: $own, parent: __layer_chain_acc }; )*
        __layer_chain_acc
    }};
    // Start from the root (), accumulating through successive shadowing
    ($($own:expr),* $(,)?) => {{
        let __layer_chain_acc = ();
        $( let __layer_chain_acc = $crate::shared::LayerChain { own: $own, parent: __layer_chain_acc }; )*
        __layer_chain_acc
    }};
}

macro_rules! from_chain {
    (($for:ty, $ctx:expr), $($tt:tt)+) => {
        <$for>::from_chain($crate::shared::layer_chain!($($tt)+), $ctx)
    };
}

pub(crate) use from_chain;
pub(crate) use instance_accessor;
pub(crate) use instance_getter;
pub(crate) use instance_method;
pub(crate) use instance_property;
pub(crate) use instance_setter;
pub(crate) use layer_chain;
pub(crate) use static_accessor;
pub(crate) use static_getter;
pub(crate) use static_method;
pub(crate) use static_property;
pub(crate) use static_setter;
