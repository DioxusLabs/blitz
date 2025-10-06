use crate::event::BlitzShellEvent;
use accesskit_winit::Adapter;
use blitz_dom::BaseDocument;
use winit::{event_loop::EventLoopProxy, window::Window};

/// State of the accessibility node tree and platform adapter.
pub struct AccessibilityState {
    /// Adapter to connect to the [`EventLoop`](`winit::event_loop::EventLoop`).
    adapter: accesskit_winit::Adapter,
}

impl AccessibilityState {
    pub fn new(window: &Window, proxy: EventLoopProxy<BlitzShellEvent>) -> Self {
        Self {
            adapter: Adapter::with_event_loop_proxy(window, proxy.clone()),
        }
    }
    pub fn update_tree(&mut self, doc: &BaseDocument) {
        self.adapter
            .update_if_active(|| doc.build_accessibility_tree());
    }
}
