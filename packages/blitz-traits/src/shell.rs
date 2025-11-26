//! Abstraction over windowing / operating system ("shell") functionality

use cursor_icon::CursorIcon;

/// A callback which wakes the event loop
pub trait EventLoopWaker: Send + Sync + 'static {
    fn wake(&self, client_id: usize);
}

#[derive(Clone)]
pub struct DummyEventLoopWaker;
impl EventLoopWaker for DummyEventLoopWaker {
    fn wake(&self, _client_id: usize) {}
}

/// Type representing an error performing a clipboard operation
// TODO: fill out with meaningful errors
pub struct ClipboardError;

/// Abstraction over windowing / operating system ("shell") functionality that allows a Blitz document
/// to access that functionality without depending on a specific shell environment.
pub trait ShellProvider: Send + Sync + 'static {
    fn request_redraw(&self) {}
    fn set_cursor(&self, icon: CursorIcon) {
        let _ = icon;
    }
    fn set_window_title(&self, title: String) {
        let _ = title;
    }
    fn set_ime_enabled(&self, is_enabled: bool) {
        let _ = is_enabled;
    }
    fn set_ime_cursor_area(&self, x: f32, y: f32, width: f32, height: f32) {
        let _ = x;
        let _ = y;
        let _ = width;
        let _ = height;
    }
    fn get_clipboard_text(&self) -> Result<String, ClipboardError> {
        Err(ClipboardError)
    }
    fn set_clipboard_text(&self, text: String) -> Result<(), ClipboardError> {
        let _ = text;
        Err(ClipboardError)
    }
    fn open_file_dialog(
        &self,
        multiple: bool,
        filter: Option<FileDialogFilter>,
    ) -> Vec<std::path::PathBuf> {
        let _ = multiple;
        let _ = filter;
        vec![]
    }
}

pub struct DummyShellProvider;
impl ShellProvider for DummyShellProvider {}

/// The system color scheme (light and dark mode)
#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub enum ColorScheme {
    #[default]
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Viewport {
    pub color_scheme: ColorScheme,
    pub window_size: (u32, u32),
    pub hidpi_scale: f32,
    pub zoom: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            window_size: (0, 0),
            hidpi_scale: 1.0,
            zoom: 1.0,
            color_scheme: ColorScheme::Light,
        }
    }
}

impl Viewport {
    pub fn new(
        physical_width: u32,
        physical_height: u32,
        scale_factor: f32,
        color_scheme: ColorScheme,
    ) -> Self {
        Self {
            window_size: (physical_width, physical_height),
            hidpi_scale: scale_factor,
            zoom: 1.0,
            color_scheme,
        }
    }

    /// Total scaling, computed as `hidpi_scale_factor * zoom`
    pub fn scale(&self) -> f32 {
        self.hidpi_scale * self.zoom
    }
    /// Same as [`scale`](Self::scale) but `f64` instead of `f32`
    pub fn scale_f64(&self) -> f64 {
        self.scale() as f64
    }

    /// Set hidpi scale factor
    pub fn set_hidpi_scale(&mut self, scale: f32) {
        self.hidpi_scale = scale;
    }

    /// Get document zoom level
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Set document zoom level (`1.0` is unzoomed)
    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom;
    }

    pub fn zoom_by(&mut self, zoom: f32) {
        self.zoom += zoom;
    }

    pub fn zoom_mut(&mut self) -> &mut f32 {
        &mut self.zoom
    }
}

/// Filter provided by the dom for an file picker
pub struct FileDialogFilter {
    pub name: String,
    pub extensions: Vec<String>,
}
