//! Shared infrastructure for JS class bindings.
//!
//! Contains error macros, native-function helpers, the `Extended<T>`
//! inheritance scheme and the member-definition macros used by the DOM layers.
#![allow(unused_imports)]
mod boa_value;
mod error;
pub mod extends;
mod macros;
mod native_function;

pub(crate) use boa_value::as_object;
pub(crate) use error::{err_node, native_error};
pub use extends::{
    Constructed, EmitOwn, ExtendLayer, Extended, ExtendedOf, HostGlobal, LayerChain, OwnBlock,
    OwnDataRegistry, RootLayer, Super, SuperDone, link_prototype, set_own_block, with_own,
    with_own_mut,
};
pub(crate) use macros::{
    from_chain, instance_accessor, instance_getter, instance_method, instance_property,
    instance_setter, layer_chain, static_accessor, static_getter, static_method, static_property,
    static_setter,
};
pub(crate) use native_function::{
    js_async_fn, js_closure, js_closure_with_captures, js_copy_closure,
    js_copy_closure_with_captures, js_fn_ptr, native_async_fn, native_closure,
    native_closure_with_captures, native_copy_closure, native_copy_closure_with_captures,
    native_fn_ptr,
};
