//! UIKit Application - manages the winit event loop and views
//!
//! This provides proper integration with the winit event loop, using custom wakers
//! to ensure the UI only updates when the DOM actually changes.

use crate::{UIKitRenderer, UIKitView, ViewConfig};
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::Arc;

use blitz_traits::net::NetWaker;
use futures_util::task::ArcWake;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::WindowId;

// =============================================================================
// Events
// =============================================================================

/// Events that can be sent to wake up the event loop
#[derive(Debug, Clone)]
pub enum UIKitEvent {
    /// Poll a specific window for updates
    Poll { window_id: WindowId },
    /// Request a redraw for a document
    RequestRedraw { doc_id: usize },
}

// =============================================================================
// Proxy
// =============================================================================

/// Proxy for sending events to the winit event loop.
///
/// This implements `NetWaker` so it can be used with blitz-net to wake up
/// the event loop when network requests complete.
#[derive(Clone)]
pub struct UIKitProxy(Arc<UIKitProxyInner>);

struct UIKitProxyInner {
    winit_proxy: EventLoopProxy,
    sender: Sender<UIKitEvent>,
}

impl UIKitProxy {
    /// Create a new proxy and event receiver.
    pub fn new(winit_proxy: EventLoopProxy) -> (Self, Receiver<UIKitEvent>) {
        let (sender, receiver) = channel();
        let proxy = Self(Arc::new(UIKitProxyInner {
            winit_proxy,
            sender,
        }));
        (proxy, receiver)
    }

    /// Wake up the event loop.
    pub fn wake_up(&self) {
        self.0.winit_proxy.wake_up();
    }

    /// Send an event to the application.
    pub fn send_event(&self, event: UIKitEvent) {
        let _ = self.0.sender.send(event);
        self.wake_up();
    }
}

impl NetWaker for UIKitProxy {
    fn wake(&self, doc_id: usize) {
        self.send_event(UIKitEvent::RequestRedraw { doc_id });
    }
}

/// Create a waker that sends Poll events to the event loop.
///
/// This allows async tasks in the VirtualDom to wake up the event loop
/// when they complete.
pub fn create_waker(proxy: &UIKitProxy, window_id: WindowId) -> std::task::Waker {
    struct WakerHandle {
        proxy: UIKitProxy,
        window_id: WindowId,
    }

    impl ArcWake for WakerHandle {
        fn wake_by_ref(arc_self: &Arc<Self>) {
            arc_self.proxy.send_event(UIKitEvent::Poll {
                window_id: arc_self.window_id,
            });
        }
    }

    futures_util::task::waker(Arc::new(WakerHandle {
        proxy: proxy.clone(),
        window_id,
    }))
}

// =============================================================================
// Application
// =============================================================================

/// The main application handler for UIKit-based Blitz apps.
///
/// This integrates with the winit event loop and manages one or more UIKitViews.
pub struct UIKitApplication {
    /// Active views by window ID
    pub views: HashMap<WindowId, UIKitView>,
    /// Pending view configurations to create on resume
    pub pending_views: Vec<ViewConfig>,
    /// Proxy for sending events
    pub proxy: UIKitProxy,
    /// Receiver for application events
    pub event_queue: Receiver<UIKitEvent>,
}

impl UIKitApplication {
    /// Create a new application with the given proxy and event queue.
    pub fn new(proxy: UIKitProxy, event_queue: Receiver<UIKitEvent>) -> Self {
        Self {
            views: HashMap::new(),
            pending_views: Vec::new(),
            proxy,
            event_queue,
        }
    }

    /// Add a view configuration to be created when the event loop starts.
    pub fn add_view(&mut self, config: ViewConfig) {
        self.pending_views.push(config);
    }

    /// Find a view by document ID.
    fn view_by_doc_id(&mut self, doc_id: usize) -> Option<&mut UIKitView> {
        self.views.values_mut().find(|v| v.doc_id() == doc_id)
    }

    /// Handle a UIKit application event.
    pub fn handle_event(&mut self, _event_loop: &dyn ActiveEventLoop, event: UIKitEvent) {
        match event {
            UIKitEvent::Poll { window_id } => {
                if let Some(view) = self.views.get_mut(&window_id) {
                    view.poll();
                }
            }
            UIKitEvent::RequestRedraw { doc_id } => {
                if let Some(view) = self.view_by_doc_id(doc_id) {
                    view.request_redraw();
                }
            }
        }
    }
}

impl ApplicationHandler for UIKitApplication {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        // Resume existing views
        for view in self.views.values_mut() {
            view.resume();
        }

        // Create pending views
        for config in self.pending_views.drain(..) {
            let view = UIKitView::init(config, event_loop, &self.proxy);
            self.views.insert(view.window_id(), view);
        }
    }

    fn destroy_surfaces(&mut self, _event_loop: &dyn ActiveEventLoop) {
        for view in self.views.values_mut() {
            view.suspend();
        }
    }

    fn resumed(&mut self, _event_loop: &dyn ActiveEventLoop) {
        // Called when app comes to foreground on iOS
        for view in self.views.values_mut() {
            view.request_redraw();
        }
    }

    fn suspended(&mut self, _event_loop: &dyn ActiveEventLoop) {
        // Called when app goes to background on iOS
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        // Handle close requests
        if matches!(event, WindowEvent::CloseRequested) {
            self.views.remove(&window_id);
            if self.views.is_empty() {
                event_loop.exit();
            }
            return;
        }

        // Forward event to the appropriate view
        if let Some(view) = self.views.get_mut(&window_id) {
            view.handle_window_event(event);
        }

        // Queue a poll for this window
        self.proxy.send_event(UIKitEvent::Poll { window_id });
    }

    fn proxy_wake_up(&mut self, event_loop: &dyn ActiveEventLoop) {
        // Process all queued events
        while let Ok(event) = self.event_queue.try_recv() {
            self.handle_event(event_loop, event);
        }
    }

    fn about_to_wait(&mut self, _event_loop: &dyn ActiveEventLoop) {
        // On iOS, we need to call request_redraw here due to winit limitations
        // But we only do it if the view has pending updates
        for view in self.views.values() {
            if view.needs_redraw() {
                view.request_redraw();
            }
        }
    }
}
