use futures_util::task::ArcWake;
use std::{any::Any, sync::Arc};
use winit::{event_loop::EventLoopProxy, window::WindowId};

#[cfg(feature = "accessibility")]
use accesskit_winit::{Event as AccessKitEvent, WindowEvent as AccessKitWindowEvent};
use blitz_dom::net::Resource;

#[derive(Debug, Clone)]
pub enum BlitzEvent {
    Poll {
        window_id: WindowId,
    },

    ResourceLoad {
        doc_id: usize,
        data: Resource,
    },

    /// An accessibility event from `accesskit`.
    #[cfg(feature = "accessibility")]
    Accessibility {
        window_id: WindowId,
        data: Arc<AccessKitWindowEvent>,
    },

    /// An arbitary event from the Blitz embedder
    Embedder(Arc<dyn Any + Send + Sync>),
}
impl BlitzEvent {
    pub fn embedder_event<T: Any + Send + Sync>(value: T) -> Self {
        let boxed = Arc::new(value) as Arc<dyn Any + Send + Sync>;
        Self::Embedder(boxed)
    }
}
impl From<(usize, Resource)> for BlitzEvent {
    fn from((doc_id, data): (usize, Resource)) -> Self {
        BlitzEvent::ResourceLoad { doc_id, data }
    }
}

#[cfg(feature = "accessibility")]
impl From<AccessKitEvent> for BlitzEvent {
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
pub fn create_waker(proxy: &EventLoopProxy<BlitzEvent>, id: WindowId) -> std::task::Waker {
    struct DomHandle {
        proxy: EventLoopProxy<BlitzEvent>,
        id: WindowId,
    }

    // this should be implemented by most platforms, but ios is missing this until
    // https://github.com/tauri-apps/wry/issues/830 is resolved
    unsafe impl Send for DomHandle {}
    unsafe impl Sync for DomHandle {}

    impl ArcWake for DomHandle {
        fn wake_by_ref(arc_self: &Arc<Self>) {
            _ = arc_self.proxy.send_event(BlitzEvent::Poll {
                window_id: arc_self.id,
            })
        }
    }

    futures_util::task::waker(Arc::new(DomHandle {
        id,
        proxy: proxy.clone(),
    }))
}
