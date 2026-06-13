use blitz_traits::shell::{DummyShellProvider, ShellProvider};

#[test]
fn window_chrome_controls_default_to_noops() {
    let provider = DummyShellProvider;
    provider.request_window_close();
    provider.set_window_minimized(true);
    provider.set_window_maximized(true);
    provider.set_window_decorations(true);
    provider.drag_window();
    assert!(!provider.is_window_maximized());
}
