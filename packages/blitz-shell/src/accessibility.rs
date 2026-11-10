use crate::{BlitzShellEvent, event::BlitzShellProxy};
use accesskit::{NodeId, Rect};
use accesskit_xplat::{Adapter, EventHandler, WindowEvent as AccessKitEvent};
use blitz_dom::BaseDocument;
use std::sync::Arc;
use winit::{
    event::WindowEvent,
    raw_window_handle::HasWindowHandle,
    window::{Window, WindowId},
};

/// State of the accessibility node tree and platform adapter.
pub struct AccessibilityState {
    // /// Adapter to connect to the [`EventLoop`](`winit::event_loop::EventLoop`).
    adapter: Adapter,
    /// The title the window was created with: winit can't read a window's
    /// title back on X11 or Wayland.
    pub(crate) window_title: String,
}

struct Handler {
    window_id: WindowId,
    proxy: BlitzShellProxy,
}
impl EventHandler for Handler {
    fn handle_accesskit_event(&self, event: AccessKitEvent) {
        self.proxy.send_event(BlitzShellEvent::Accessibility {
            window_id: self.window_id,
            data: Arc::new(event),
        });
    }
}

impl AccessibilityState {
    pub fn new(window: &dyn Window, proxy: BlitzShellProxy) -> Self {
        let window_id = window.id();
        Self {
            adapter: Adapter::with_combined_handler(
                #[cfg(target_os = "android")]
                &crate::current_android_app(),
                #[cfg(not(target_os = "android"))]
                window.window_handle().unwrap().as_raw(),
                Arc::new(Handler { window_id, proxy }),
            ),
            window_title: String::new(),
        }
    }
    /// Sends the document's tree, its window node named after the window:
    /// the document's `<title>`, or the title the window was created with.
    pub fn update_tree(&mut self, doc: &BaseDocument, window: &dyn Window) {
        let window_title = &self.window_title;
        self.adapter.update_if_active(|| {
            let mut update = doc.build_accessibility_tree();
            let title = doc
                .find_title_node()
                .map(|node| node.text_content())
                .filter(|title| !title.is_empty())
                .or_else(|| (!window_title.is_empty()).then(|| window_title.clone()))
                .unwrap_or_else(|| window.title());
            if !title.is_empty()
                && let Some((_, root)) = update
                    .nodes
                    .iter_mut()
                    .find(|(id, _)| *id == NodeId(u64::MAX))
            {
                root.set_label(title);
            }
            update
        });
    }

    /// Allows reacting to window events.
    ///
    /// This must be called whenever a new window event is received
    /// and before it is handled by the application.
    pub fn process_window_event(&mut self, window: &dyn Window, event: &WindowEvent) {
        match event {
            WindowEvent::Focused(is_focused) => {
                self.adapter.set_focus(*is_focused);
            }
            WindowEvent::Moved(_) | WindowEvent::SurfaceResized(_) => {
                let outer_position: (_, _) = window
                    .outer_position()
                    .unwrap_or_default()
                    .cast::<f64>()
                    .into();
                let outer_size: (_, _) = window.outer_size().cast::<f64>().into();
                let inner_position: (_, _) = window.surface_position().cast::<f64>().into();
                let inner_size: (_, _) = window.surface_size().cast::<f64>().into();

                self.adapter.set_window_bounds(
                    Rect::from_origin_size(outer_position, outer_size),
                    Rect::from_origin_size(inner_position, inner_size),
                )
            }
            _ => (),
        }
    }
}
