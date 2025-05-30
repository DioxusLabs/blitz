use cursor_icon::CursorIcon;

pub trait ShellProvider {
    fn request_redraw(&self) {}
    fn set_cursor(&self, icon: CursorIcon) {
        let _ = icon;
    }
}

pub struct DummyShellProvider;
impl ShellProvider for DummyShellProvider {}
