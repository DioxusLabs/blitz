use crate::Document;
use blitz_traits::{Devtools, Viewport, WasmNotSendSync};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::sync::Arc;

pub trait BlitzWindowHandle: HasWindowHandle + HasDisplayHandle + WasmNotSendSync {}
impl<T: HasWindowHandle + HasDisplayHandle + WasmNotSendSync> BlitzWindowHandle for T {}

pub trait DocumentRenderer {
    fn new(window: Arc<dyn BlitzWindowHandle>) -> Self;
    fn is_active(&self) -> bool;
    fn resume(&mut self, viewport: &Viewport);
    fn suspend(&mut self);

    /// Adjust the viewport
    fn set_size(&mut self, physical_width: u32, physical_height: u32);

    fn render(&mut self, doc: &Document, scale: f64, width: u32, height: u32, devtools: Devtools);
}
