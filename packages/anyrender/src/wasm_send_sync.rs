//! Traits that imply `Send`/`Sync` only on non-wasm platforms. For interop with wgpu.

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

/// A raw window handle that is `WasmNotSendSync`. For interop with wgpu.
pub trait WindowHandle: HasWindowHandle + HasDisplayHandle + WasmNotSendSync {}
impl<T: HasWindowHandle + HasDisplayHandle + WasmNotSendSync> WindowHandle for T {}

/// Trait that implies `Send` and `Sync` on non-wasm platforms
pub trait WasmNotSendSync: WasmNotSend + WasmNotSync {}
impl<T: WasmNotSend + WasmNotSync> WasmNotSendSync for T {}

/// Trait that implies `Send` on non-wasm platforms
#[cfg(not(target_arch = "wasm32"))]
pub trait WasmNotSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send> WasmNotSend for T {}
/// Trait that implies `Send` on non-wasm platforms
#[cfg(target_arch = "wasm32")]
pub trait WasmNotSend {}
#[cfg(target_arch = "wasm32")]
impl<T> WasmNotSend for T {}

/// Trait that implies `Sync` on non-wasm platforms
#[cfg(not(target_arch = "wasm32"))]
pub trait WasmNotSync: Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Sync> WasmNotSync for T {}
/// Trait that implies `Sync` on non-wasm platforms
#[cfg(target_arch = "wasm32")]
pub trait WasmNotSync {}
#[cfg(target_arch = "wasm32")]
impl<T> WasmNotSync for T {}
