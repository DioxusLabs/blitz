pub mod net;

mod devtools;
pub use devtools::Devtools;

mod viewport;
pub use viewport::{ColorScheme, Viewport};

pub use send_sync::{WasmNotSend, WasmNotSendSync, WasmNotSync};

#[doc(hidden)]
mod send_sync {
    pub trait WasmNotSendSync: WasmNotSend + WasmNotSync {}
    impl<T: WasmNotSend + WasmNotSync> WasmNotSendSync for T {}

    #[cfg(not(target_arch = "wasm32"))]
    pub trait WasmNotSend: Send {}
    #[cfg(not(target_arch = "wasm32"))]
    impl<T: Send> WasmNotSend for T {}
    #[cfg(target_arch = "wasm32")]
    pub trait WasmNotSend {}
    #[cfg(target_arch = "wasm32")]
    impl<T> WasmNotSend for T {}

    #[cfg(not(target_arch = "wasm32"))]
    pub trait WasmNotSync: Sync {}
    #[cfg(not(target_arch = "wasm32"))]
    impl<T: Sync> WasmNotSync for T {}
    #[cfg(target_arch = "wasm32")]
    pub trait WasmNotSync {}
    #[cfg(target_arch = "wasm32")]
    impl<T> WasmNotSync for T {}
}
