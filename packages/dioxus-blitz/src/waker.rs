use blitz_dom::DocumentLike;
use futures_util::task::ArcWake;
use std::sync::Arc;
use winit::{
    event_loop::{ActiveEventLoop, EventLoopProxy},
    window::WindowId,
};

#[cfg(feature = "accessibility")]
use accesskit_winit::Event as AccessibilityEvent;
use accesskit_winit::WindowEvent as AccessibilityWindowEvent;

use crate::application::Application;

// pub trait CustomEventHandler<Doc: DocumentLike>: Sized + Send {
//     fn handle(self, app: &mut Application<Doc, Self>, event_loop: &ActiveEventLoop);
// }

// impl<Doc: DocumentLike> CustomEventHandler<Doc> for () {
//     fn handle(self, app: &mut Application<Doc, Self>, event_loop: &ActiveEventLoop) {
//         todo!()
//     }
// }

#[derive(Debug, Clone)]
pub enum BlitzWindowId {
    AllWindows,
    SpecificWindow(WindowId),
}

#[derive(Debug, Clone)]
pub enum BlitzEvent<DocumentEvent: 'static> {
    Window {
        window_id: BlitzWindowId,
        data: BlitzWindowEvent<DocumentEvent>,
    },
    Exit,
}

#[cfg(feature = "accessibility")]
impl<T> From<AccessibilityEvent> for BlitzEvent<T> {
    fn from(value: AccessibilityEvent) -> Self {
        let window_event = BlitzWindowEvent::Accessibility(Arc::new(value.window_event));
        Self::Window {
            window_id: BlitzWindowId::SpecificWindow(value.window_id),
            data: window_event,
        }
    }
}

#[derive(Debug, Clone)]
pub enum BlitzWindowEvent<DocumentEvent> {
    Poll,

    DocumentEvent(DocumentEvent),

    /// An accessibility event from `accesskit`.
    #[cfg(feature = "accessibility")]
    Accessibility(Arc<AccessibilityWindowEvent>),
    // NewWindow,
    // CloseWindow,
}

/// Create a waker that will send a poll event to the event loop.
///
/// This lets the VirtualDom "come up for air" and process events while the main thread is blocked by the WebView.
///
/// All other IO lives in the Tokio runtime,
pub fn create_waker<D: 'static>(
    proxy: &EventLoopProxy<BlitzEvent<D>>,
    id: WindowId,
) -> std::task::Waker {
    struct DomHandle<D: 'static> {
        proxy: EventLoopProxy<BlitzEvent<D>>,
        id: WindowId,
    }

    // this should be implemented by most platforms, but ios is missing this until
    // https://github.com/tauri-apps/wry/issues/830 is resolved
    unsafe impl<D> Send for DomHandle<D> {}
    unsafe impl<D> Sync for DomHandle<D> {}

    impl<D> ArcWake for DomHandle<D> {
        fn wake_by_ref(arc_self: &Arc<Self>) {
            _ = arc_self.proxy.send_event(BlitzEvent::Window {
                data: BlitzWindowEvent::Poll,
                window_id: BlitzWindowId::SpecificWindow(arc_self.id),
            })
        }
    }

    futures_util::task::waker(Arc::new(DomHandle {
        id,
        proxy: proxy.clone(),
    }))
}
