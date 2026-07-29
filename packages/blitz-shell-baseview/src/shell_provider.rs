use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Wake, Waker};

use blitz_traits::net::NetWaker;
use blitz_traits::shell::{ClipboardError, ShellProvider};
use cursor_icon::CursorIcon;

/// State shared between the [`ShellProvider`] handed to the document (which must be
/// `Send + Sync` and may be invoked from any thread) and the baseview `WindowHandler`
/// (which runs on the window thread and holds the `WindowContext`).
///
/// baseview has no cross-thread event-loop wakeup mechanism, but it polls
/// `WindowHandler::on_frame` at frame rate, so simple flags checked each frame are
/// sufficient.
#[derive(Default)]
pub(crate) struct SharedShellState {
    /// A redraw of the document has been requested
    pub(crate) redraw_requested: AtomicBool,
    /// The document should be polled (async wakeup from e.g. a net provider)
    pub(crate) poll_requested: AtomicBool,
    /// The window should be closed
    pub(crate) close_requested: AtomicBool,
    /// A pending cursor change. The outer `Option` is "is there a pending change",
    /// the inner `Option` is the cursor icon (`None` = hide the cursor).
    pub(crate) cursor: Mutex<Option<Option<CursorIcon>>>,
}

impl SharedShellState {
    pub(crate) fn request_redraw(&self) {
        self.redraw_requested.store(true, Ordering::Release);
    }

    pub(crate) fn take(&self, flag: &AtomicBool) -> bool {
        flag.swap(false, Ordering::AcqRel)
    }
}

/// A [`ShellProvider`] implementation backed by [`SharedShellState`] flags that are
/// applied by the baseview `WindowHandler` on the next frame.
pub(crate) struct BaseviewShellProvider(pub(crate) Arc<SharedShellState>);

impl ShellProvider for BaseviewShellProvider {
    fn request_redraw(&self) {
        self.0.request_redraw();
    }

    fn set_cursor(&self, icon: Option<CursorIcon>) {
        *self.0.cursor.lock().unwrap() = Some(icon);
    }

    fn request_window_close(&self) {
        self.0.close_requested.store(true, Ordering::Release);
    }

    fn set_clipboard_text(&self, text: String) -> Result<(), ClipboardError> {
        baseview::copy_to_clipboard(&text);
        Ok(())
    }

    // baseview does not support reading the clipboard, window titles, minimization,
    // maximization, decorations, window dragging, IME or file dialogs: the default
    // (no-op) implementations are used for those methods. This is usually correct
    // for the audio plugin use-case, where the host owns the window.
}

impl NetWaker for BaseviewShellProvider {
    fn wake(&self, _client_id: usize) {
        self.0.poll_requested.store(true, Ordering::Release);
    }
}

/// Waker used for `Document::poll`. Requests a poll on the next frame.
struct PollWaker(Arc<SharedShellState>);

impl Wake for PollWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.poll_requested.store(true, Ordering::Release);
    }
}

pub(crate) fn create_waker(shared: &Arc<SharedShellState>) -> Waker {
    Waker::from(Arc::new(PollWaker(Arc::clone(shared))))
}
