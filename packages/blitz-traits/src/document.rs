use crate::{Devtools, DomEvent, Viewport, WasmNotSendSync};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::any::Any;
use std::sync::Arc;

pub trait Document: AsRef<Self::Doc> + AsMut<Self::Doc> + Into<Self::Doc> + 'static {
    type Doc: 'static;

    fn poll(&mut self, _cx: std::task::Context) -> bool {
        // Default implementation does nothing
        false
    }

    fn handle_event(&mut self, _event: DomEvent) {
        // Default implementation does nothing
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn id(&self) -> usize;
}

pub trait BlitzWindowHandle: HasWindowHandle + HasDisplayHandle + WasmNotSendSync {}
impl<T: HasWindowHandle + HasDisplayHandle + WasmNotSendSync> BlitzWindowHandle for T {}

pub trait DocumentRenderer {
    type Doc: 'static;

    fn new(window: Arc<dyn BlitzWindowHandle>) -> Self;
    fn is_active(&self) -> bool;
    fn resume(&mut self, viewport: &Viewport);
    fn suspend(&mut self);

    /// Adjust the viewport
    fn set_size(&mut self, physical_width: u32, physical_height: u32);

    fn render(&mut self, doc: &Self::Doc, scale: f64, width: u32, height: u32, devtools: Devtools);
}
