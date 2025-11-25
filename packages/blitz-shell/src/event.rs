use blitz_traits::navigation::NavigationOptions;
use futures_util::task::ArcWake;
use std::{any::Any, sync::Arc};
use winit::{event_loop::EventLoopProxy, window::WindowId};

#[cfg(feature = "accessibility")]
use accesskit_winit::{Event as AccessKitEvent, WindowEvent as AccessKitWindowEvent};

#[derive(Debug, Clone)]
pub enum BlitzShellEvent {
    Poll {
        window_id: WindowId,
    },

    RequestRedraw {
        doc_id: usize,
    },

    /// An accessibility event from `accesskit`.
    #[cfg(feature = "accessibility")]
    Accessibility {
        window_id: WindowId,
        data: Arc<AccessKitWindowEvent>,
    },

    /// An arbitary event from the Blitz embedder
    Embedder(Arc<dyn Any + Send + Sync>),

    /// Navigate to another URL (triggered by e.g. clicking a link)
    Navigate(Box<NavigationOptions>),

    /// Navigate to another URL (triggered by e.g. clicking a link)
    NavigationLoad {
        url: String,
        contents: String,
        retain_scroll_position: bool,
        is_md: bool,
    },
}
impl BlitzShellEvent {
    pub fn embedder_event<T: Any + Send + Sync>(value: T) -> Self {
        let boxed = Arc::new(value) as Arc<dyn Any + Send + Sync>;
        Self::Embedder(boxed)
    }
}

#[cfg(feature = "accessibility")]
impl From<AccessKitEvent> for BlitzShellEvent {
    fn from(value: AccessKitEvent) -> Self {
        Self::Accessibility {
            window_id: value.window_id,
            data: Arc::new(value.window_event),
        }
    }
}

/// Create a waker that will send a poll event to the event loop.
///
/// This lets the VirtualDom "come up for air" and process events while the main thread is blocked by the WebView.
///
/// All other IO lives in the Tokio runtime,
pub fn create_waker(proxy: &EventLoopProxy<BlitzShellEvent>, id: WindowId) -> std::task::Waker {
    struct DomHandle {
        proxy: EventLoopProxy<BlitzShellEvent>,
        id: WindowId,
    }

    // this should be implemented by most platforms, but ios is missing this until
    // https://github.com/tauri-apps/wry/issues/830 is resolved
    unsafe impl Send for DomHandle {}
    unsafe impl Sync for DomHandle {}

    impl ArcWake for DomHandle {
        fn wake_by_ref(arc_self: &Arc<Self>) {
            _ = arc_self.proxy.send_event(BlitzShellEvent::Poll {
                window_id: arc_self.id,
            })
        }
    }

    futures_util::task::waker(Arc::new(DomHandle {
        id,
        proxy: proxy.clone(),
    }))
}
