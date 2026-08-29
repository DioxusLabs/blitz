#![allow(unused)]

macro_rules! js_fn_ptr {
    ($function: expr, $realm: expr) => {
        boa_engine::native_function::NativeFunction::from_fn_ptr($function).to_js_function($realm)
    };
}
macro_rules! js_async_fn {
    ($f: expr, $realm: expr) => {
        boa_engine::native_function::NativeFunction::from_async_fn($f).to_js_function($realm)
    };
}
macro_rules! js_copy_closure {
    ($closure: expr, $realm: expr) => {
        boa_engine::native_function::NativeFunction::from_copy_closure($closure)
            .to_js_function($realm)
    };
}
macro_rules! js_copy_closure_with_captures {
    ($closure: expr, $captures: expr, $realm: expr) => {
        boa_engine::native_function::NativeFunction::from_copy_closure_with_captures(
            $closure, $captures,
        )
        .to_js_function($realm)
    };
}
macro_rules! js_closure {
    ($closure: expr, $realm: expr) => {
        boa_engine::native_function::NativeFunction::from_closure($closure).to_js_function($realm)
    };
}
macro_rules! js_closure_with_captures {
    ($closure: expr, $captures: expr, $realm: expr) => {
        boa_engine::native_function::NativeFunction::from_closure_with_captures($closure, $captures)
            .to_js_function($realm)
    };
}

pub(crate) use js_async_fn;
pub(crate) use js_closure;
pub(crate) use js_closure_with_captures;
pub(crate) use js_copy_closure;
pub(crate) use js_copy_closure_with_captures;
pub(crate) use js_fn_ptr;

macro_rules! native_fn_ptr {
    ($function: expr) => {
        boa_engine::native_function::NativeFunction::from_fn_ptr($function)
    };
}
macro_rules! native_async_fn {
    ($f: expr) => {
        boa_engine::native_function::NativeFunction::from_async_fn($f)
    };
}
macro_rules! native_copy_closure {
    ($closure: expr) => {
        boa_engine::native_function::NativeFunction::from_copy_closure($closure)
    };
}
macro_rules! native_copy_closure_with_captures {
    ($closure: expr, $captures: expr) => {
        boa_engine::native_function::NativeFunction::from_copy_closure_with_captures(
            $closure, $captures,
        )
    };
}
macro_rules! native_closure {
    ($closure: expr) => {
        boa_engine::native_function::NativeFunction::from_closure($closure)
    };
}
macro_rules! native_closure_with_captures {
    ($closure: expr, $captures: expr) => {
        boa_engine::native_function::NativeFunction::from_closure_with_captures($closure, $captures)
    };
}

pub(crate) use native_async_fn;
pub(crate) use native_closure;
pub(crate) use native_closure_with_captures;
pub(crate) use native_copy_closure;
pub(crate) use native_copy_closure_with_captures;
pub(crate) use native_fn_ptr;
