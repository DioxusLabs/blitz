use cursor_icon::CursorIcon;

pub trait ShellProvider {
    fn request_redraw(&self) {}
    fn set_cursor(&self, icon: CursorIcon) {
        let _ = icon;
    }
    fn set_window_title(&self, title: String) {
        let _ = title;
    }
}

pub struct DummyShellProvider;
impl ShellProvider for DummyShellProvider {}
