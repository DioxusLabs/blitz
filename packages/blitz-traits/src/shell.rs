use cursor_icon::CursorIcon;

/// Type representing an error performing a clipboard operation
/// TODO: fill out with meaningful errors
pub struct ClipboardError;

pub trait ShellProvider {
    fn request_redraw(&self) {}
    fn set_cursor(&self, icon: CursorIcon) {
        let _ = icon;
    }
    fn set_window_title(&self, title: String) {
        let _ = title;
    }
    fn get_clipboard_text(&self) -> Result<String, ClipboardError> {
        Err(ClipboardError)
    }
    fn set_clipboard_text(&self, text: String) -> Result<(), ClipboardError> {
        let _ = text;
        Err(ClipboardError)
    }
}

pub struct DummyShellProvider;
impl ShellProvider for DummyShellProvider {}
