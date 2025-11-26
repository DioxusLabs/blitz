use blitz_traits::navigation::NavigationOptions;
use blitz_traits::shell::EventLoopWaker;
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
    impl ArcWake for DomHandle {
        fn wake_by_ref(arc_self: &Arc<Self>) {
            _ = arc_self.proxy.send_event(BlitzShellEvent::Poll {
                window_id: arc_self.id,
            })
        }
    }

    let proxy = proxy.clone();
    futures_util::task::waker(Arc::new(DomHandle { id, proxy }))
}

/// A EventLoopWaker that wakes up our winit event loop
pub struct BlitzShellWaker<F: Fn(usize) -> BlitzShellEvent + Send + Sync + 'static> {
    proxy: EventLoopProxy<BlitzShellEvent>,
    cb: F,
}

impl<F: Fn(usize) -> BlitzShellEvent + Send + Sync + 'static> BlitzShellWaker<F> {
    pub fn new(proxy: EventLoopProxy<BlitzShellEvent>, cb: F) -> Self {
        Self { proxy, cb }
    }

    pub fn shared(proxy: EventLoopProxy<BlitzShellEvent>, cb: F) -> Arc<dyn EventLoopWaker> {
        Arc::new(Self::new(proxy, cb)) as _
    }
}

impl BlitzShellWaker<fn(usize) -> BlitzShellEvent> {
    pub fn net_waker(proxy: EventLoopProxy<BlitzShellEvent>) -> Arc<dyn EventLoopWaker> {
        BlitzShellWaker::shared(proxy, |doc_id| BlitzShellEvent::RequestRedraw { doc_id })
    }

    pub fn devtools_waker(proxy: EventLoopProxy<BlitzShellEvent>) -> Arc<dyn EventLoopWaker> {
        BlitzShellWaker::shared(proxy, |_| BlitzShellEvent::ProcessDevtoolMessages)
    }
}

impl<F: Fn(usize) -> BlitzShellEvent + Send + Sync + 'static> EventLoopWaker
    for BlitzShellWaker<F>
{
    fn wake(&self, doc_id: usize) {
        self.proxy.send_event((self.cb)(doc_id)).unwrap()
    }
}
