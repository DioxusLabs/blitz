use blitz_traits::navigation::NavigationOptions;
use blitz_traits::net::NetWaker;
use futures_util::task::ArcWake;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::{any::Any, sync::Arc};
use winit::{event_loop::EventLoopProxy, window::WindowId};

// #[cfg(feature = "accessibility")]
// use accesskit_winit::{Event as AccessKitEvent, WindowEvent as AccessKitWindowEvent};

#[derive(Debug, Clone)]
pub enum BlitzShellEvent {
    Poll {
        window_id: WindowId,
    },

    RequestRedraw {
        doc_id: usize,
    },

    /// An accessibility event from `accesskit`.
    // #[cfg(feature = "accessibility")]
    // Accessibility {
    //     window_id: WindowId,
    //     data: Arc<AccessKitWindowEvent>,
    // },

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

// #[cfg(feature = "accessibility")]
// impl From<AccessKitEvent> for BlitzShellEvent {
//     fn from(value: AccessKitEvent) -> Self {
//         Self::Accessibility {
//             window_id: value.window_id,
//             data: Arc::new(value.window_event),
//         }
//     }
// }

#[derive(Clone)]
pub struct BlitzShellProxy(Arc<BlitzShellProxyInner>);
pub struct BlitzShellProxyInner {
    winit_proxy: EventLoopProxy,
    sender: Sender<BlitzShellEvent>,
}

impl BlitzShellProxy {
    pub fn new(winit_proxy: EventLoopProxy) -> (Self, Receiver<BlitzShellEvent>) {
        let (sender, receiver) = channel();
        let proxy = Self(Arc::new(BlitzShellProxyInner {
            winit_proxy,
            sender,
        }));
        (proxy, receiver)
    }

    pub fn wake_up(&self) {
        self.0.winit_proxy.wake_up();
    }
    pub fn send_event(&self, event: impl Into<BlitzShellEvent>) {
        self.send_event_impl(event.into());
    }
    fn send_event_impl(&self, event: BlitzShellEvent) {
        let _ = self.0.sender.send(event);
        self.wake_up();
    }
}

impl NetWaker for BlitzShellProxy {
    fn wake(&self, client_id: usize) {
        self.send_event_impl(BlitzShellEvent::RequestRedraw { doc_id: client_id })
    }
}

/// Create a waker that will send a poll event to the event loop.
///
/// This lets the VirtualDom "come up for air" and process events while the main thread is blocked by the WebView.
///
/// All other IO lives in the Tokio runtime,
pub fn create_waker(proxy: &BlitzShellProxy, id: WindowId) -> std::task::Waker {
    struct DomHandle {
        proxy: BlitzShellProxy,
        id: WindowId,
    }
    impl ArcWake for DomHandle {
        fn wake_by_ref(arc_self: &Arc<Self>) {
            let event = BlitzShellEvent::Poll {
                window_id: arc_self.id,
            };
            arc_self.proxy.send_event(event)
        }
    }

    let proxy = proxy.clone();
    futures_util::task::waker(Arc::new(DomHandle { id, proxy }))
}
