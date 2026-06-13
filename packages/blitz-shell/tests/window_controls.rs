//! `BlitzShellProvider`'s window-chrome controls drive the winit window and
//! event loop, so a document can implement a custom titlebar (drag region +
//! minimize/maximize/close buttons) on a frameless window.

use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use raw_window_handle as rwh_06;

use blitz_shell::{BlitzShellEvent, BlitzShellProvider, BlitzShellProxy};
use blitz_traits::shell::ShellProvider;
use winit::cursor::Cursor;
use winit::dpi::{PhysicalInsets, PhysicalPosition, PhysicalSize, Position, Size};
use winit::error::RequestError;
use winit::event_loop::{EventLoopProxy, EventLoopProxyProvider};
use winit::icon::Icon;
use winit::monitor::{Fullscreen, MonitorHandle};
use winit::window::{
    CursorGrabMode, ImeCapabilities, ImeRequest, ImeRequestError, ResizeDirection, Theme,
    UserAttentionType, Window, WindowButtons, WindowId, WindowLevel,
};

#[derive(Debug)]
struct StubWaker;
impl EventLoopProxyProvider for StubWaker {
    fn wake_up(&self) {}
}

fn shell_proxy() -> (BlitzShellProxy, Receiver<BlitzShellEvent>) {
    BlitzShellProxy::new(EventLoopProxy::new(Arc::new(StubWaker)))
}

const MOCK_WINDOW_ID: usize = 42;

#[derive(Debug, Default)]
struct MockWindow {
    calls: Mutex<Vec<String>>,
    maximized: Mutex<bool>,
}

impl MockWindow {
    fn record(&self, call: &str) {
        self.calls.lock().unwrap().push(call.to_string());
    }
}

impl rwh_06::HasDisplayHandle for MockWindow {
    fn display_handle(&self) -> Result<rwh_06::DisplayHandle<'_>, rwh_06::HandleError> {
        Err(rwh_06::HandleError::Unavailable)
    }
}
impl rwh_06::HasWindowHandle for MockWindow {
    fn window_handle(&self) -> Result<rwh_06::WindowHandle<'_>, rwh_06::HandleError> {
        Err(rwh_06::HandleError::Unavailable)
    }
}

impl Window for MockWindow {
    fn id(&self) -> WindowId {
        WindowId::from_raw(MOCK_WINDOW_ID)
    }
    fn set_minimized(&self, minimized: bool) {
        self.record(&format!("set_minimized({minimized})"));
    }
    fn is_minimized(&self) -> Option<bool> {
        None
    }
    fn set_maximized(&self, maximized: bool) {
        self.record(&format!("set_maximized({maximized})"));
        *self.maximized.lock().unwrap() = maximized;
    }
    fn is_maximized(&self) -> bool {
        *self.maximized.lock().unwrap()
    }
    fn drag_window(&self) -> Result<(), RequestError> {
        self.record("drag_window");
        Ok(())
    }
    fn set_decorations(&self, decorations: bool) {
        self.record(&format!("set_decorations({decorations})"));
    }

    // Everything below is unused by these tests
    fn scale_factor(&self) -> f64 {
        unimplemented!()
    }
    fn is_decorated(&self) -> bool {
        unimplemented!()
    }
    fn request_redraw(&self) {
        unimplemented!()
    }
    fn pre_present_notify(&self) {
        unimplemented!()
    }
    fn reset_dead_keys(&self) {
        unimplemented!()
    }
    fn surface_position(&self) -> PhysicalPosition<i32> {
        unimplemented!()
    }
    fn outer_position(&self) -> Result<PhysicalPosition<i32>, RequestError> {
        unimplemented!()
    }
    fn set_outer_position(&self, _position: Position) {
        unimplemented!()
    }
    fn surface_size(&self) -> PhysicalSize<u32> {
        unimplemented!()
    }
    fn request_surface_size(&self, _size: Size) -> Option<PhysicalSize<u32>> {
        unimplemented!()
    }
    fn outer_size(&self) -> PhysicalSize<u32> {
        unimplemented!()
    }
    fn safe_area(&self) -> PhysicalInsets<u32> {
        unimplemented!()
    }
    fn set_min_surface_size(&self, _min_size: Option<Size>) {
        unimplemented!()
    }
    fn set_max_surface_size(&self, _max_size: Option<Size>) {
        unimplemented!()
    }
    fn surface_resize_increments(&self) -> Option<PhysicalSize<u32>> {
        unimplemented!()
    }
    fn set_surface_resize_increments(&self, _increments: Option<Size>) {
        unimplemented!()
    }
    fn set_title(&self, _title: &str) {
        unimplemented!()
    }
    fn set_transparent(&self, _transparent: bool) {
        unimplemented!()
    }
    fn set_blur(&self, _blur: bool) {
        unimplemented!()
    }
    fn set_visible(&self, _visible: bool) {
        unimplemented!()
    }
    fn is_visible(&self) -> Option<bool> {
        unimplemented!()
    }
    fn set_resizable(&self, _resizable: bool) {
        unimplemented!()
    }
    fn is_resizable(&self) -> bool {
        unimplemented!()
    }
    fn set_enabled_buttons(&self, _buttons: WindowButtons) {
        unimplemented!()
    }
    fn enabled_buttons(&self) -> WindowButtons {
        unimplemented!()
    }
    fn set_fullscreen(&self, _fullscreen: Option<Fullscreen>) {
        unimplemented!()
    }
    fn fullscreen(&self) -> Option<Fullscreen> {
        unimplemented!()
    }
    fn set_window_level(&self, _level: WindowLevel) {
        unimplemented!()
    }
    fn set_window_icon(&self, _window_icon: Option<Icon>) {
        unimplemented!()
    }
    fn request_ime_update(&self, _request: ImeRequest) -> Result<(), ImeRequestError> {
        unimplemented!()
    }
    fn ime_capabilities(&self) -> Option<ImeCapabilities> {
        unimplemented!()
    }
    fn focus_window(&self) {
        unimplemented!()
    }
    fn has_focus(&self) -> bool {
        unimplemented!()
    }
    fn request_user_attention(&self, _request_type: Option<UserAttentionType>) {
        unimplemented!()
    }
    fn set_theme(&self, _theme: Option<Theme>) {
        unimplemented!()
    }
    fn theme(&self) -> Option<Theme> {
        unimplemented!()
    }
    fn set_content_protected(&self, _protected: bool) {
        unimplemented!()
    }
    fn title(&self) -> String {
        unimplemented!()
    }
    fn set_cursor(&self, _cursor: Cursor) {
        unimplemented!()
    }
    fn set_cursor_position(&self, _position: Position) -> Result<(), RequestError> {
        unimplemented!()
    }
    fn set_cursor_grab(&self, _mode: CursorGrabMode) -> Result<(), RequestError> {
        unimplemented!()
    }
    fn set_cursor_visible(&self, _visible: bool) {
        unimplemented!()
    }
    fn drag_resize_window(&self, _direction: ResizeDirection) -> Result<(), RequestError> {
        unimplemented!()
    }
    fn show_window_menu(&self, _position: Position) {
        unimplemented!()
    }
    fn set_cursor_hittest(&self, _hittest: bool) -> Result<(), RequestError> {
        unimplemented!()
    }
    fn current_monitor(&self) -> Option<MonitorHandle> {
        unimplemented!()
    }
    fn available_monitors(&self) -> Box<dyn Iterator<Item = MonitorHandle>> {
        unimplemented!()
    }
    fn primary_monitor(&self) -> Option<MonitorHandle> {
        unimplemented!()
    }
    fn rwh_06_display_handle(&self) -> &dyn rwh_06::HasDisplayHandle {
        self
    }
    fn rwh_06_window_handle(&self) -> &dyn rwh_06::HasWindowHandle {
        self
    }
}

fn provider_with_mock() -> (Arc<MockWindow>, BlitzShellProvider, Receiver<BlitzShellEvent>) {
    let window = Arc::new(MockWindow::default());
    let (proxy, receiver) = shell_proxy();
    let provider = BlitzShellProvider::new(window.clone() as Arc<dyn Window>, proxy);
    (window, provider, receiver)
}

#[test]
fn minimize_maximize_and_drag_forward_to_the_window() {
    let (window, provider, _receiver) = provider_with_mock();

    provider.set_window_minimized(true);
    provider.drag_window();
    assert!(!provider.is_window_maximized());
    provider.set_window_maximized(true);
    assert!(provider.is_window_maximized());
    provider.set_window_decorations(false);

    let calls = window.calls.lock().unwrap();
    assert_eq!(
        *calls,
        vec![
            "set_minimized(true)",
            "drag_window",
            "set_maximized(true)",
            "set_decorations(false)"
        ]
    );
}

#[test]
fn request_window_close_sends_a_close_event_for_the_window() {
    let (_window, provider, receiver) = provider_with_mock();

    provider.request_window_close();

    let event = receiver.try_recv().expect("a close event should be queued");
    match event {
        BlitzShellEvent::CloseWindow { window_id } => {
            assert_eq!(window_id, WindowId::from_raw(MOCK_WINDOW_ID));
        }
        _ => panic!("expected CloseWindow event"),
    }
}
