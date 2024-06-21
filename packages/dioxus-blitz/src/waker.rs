use futures_util::task::ArcWake;
use std::sync::Arc;
use winit::{event_loop::EventLoopProxy, window::WindowId};

#[cfg(feature = "accesskit")]
use accesskit_winit::Event as AccessibilityEvent;

#[derive(Debug, Clone)]
pub enum UserEvent {
    Window {
        window_id: WindowId,
        data: EventData,
    },

    /// An accessibility event from `accesskit`.
    #[cfg(feature = "accesskit")]
    Accessibility(Arc<AccessibilityEvent>),
    
    /// A hotreload event, basically telling us to update our templates.
    #[cfg(all(
        feature = "hot-reload",
        debug_assertions,
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    HotReloadEvent(dioxus_hot_reload::HotReloadMsg),
}

#[cfg(feature = "accesskit")]
impl From<AccessibilityEvent> for UserEvent {
    fn from(value: AccessibilityEvent) -> Self {
        Self::Accessibility(Arc::new(value))
    }
}


#[derive(Debug, Clone)]
pub enum EventData {
    Poll,
    // NewWindow,
    // CloseWindow,
}

/// Create a waker that will send a poll event to the event loop.
///
/// This lets the VirtualDom "come up for air" and process events while the main thread is blocked by the WebView.
///
/// All other IO lives in the Tokio runtime,
pub fn tao_waker(proxy: &EventLoopProxy<UserEvent>, id: WindowId) -> std::task::Waker {
    struct DomHandle {
        proxy: EventLoopProxy<UserEvent>,
        id: WindowId,
    }

    // this should be implemented by most platforms, but ios is missing this until
    // https://github.com/tauri-apps/wry/issues/830 is resolved
    unsafe impl Send for DomHandle {}
    unsafe impl Sync for DomHandle {}

    impl ArcWake for DomHandle {
        fn wake_by_ref(arc_self: &Arc<Self>) {
            _ = arc_self.proxy.send_event(UserEvent::Window {
                data: EventData::Poll,
                window_id: arc_self.id,
            })
        }
    }

    futures_util::task::waker(Arc::new(DomHandle {
        id,
        proxy: proxy.clone(),
    }))
}
