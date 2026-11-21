//! Abstraction over windowing / operating system ("shell") functionality

use cursor_icon::CursorIcon;

/// Type representing an error performing a clipboard operation
// TODO: fill out with meaningful errors
pub struct ClipboardError;

/// Abstraction over windowing / operating system ("shell") functionality that allows a Blitz document
/// to access that functionality without depending on a specific shell environment.
pub trait ShellProvider: Send + Sync + 'static {
    fn request_redraw(&self) {}
    fn set_cursor(&self, icon: Option<CursorIcon>) {
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
    /// Request clipboard text, delivered asynchronously to the document as a `UiEvent`.
    /// The text is applied to whichever text input is focused when it arrives.
    fn request_clipboard_text(&self) -> Result<(), ClipboardError> {
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

    // Window chrome controls, for documents that draw their own titlebar
    // (e.g. a frameless window with an HTML titlebar).
    fn request_window_close(&self) {}
    fn set_window_minimized(&self, minimized: bool) {
        let _ = minimized;
    }
    fn set_window_maximized(&self, maximized: bool) {
        let _ = maximized;
    }
    fn is_window_maximized(&self) -> bool {
        false
    }
    fn set_window_decorations(&self, decorations: bool) {
        let _ = decorations;
    }
    /// Begin an interactive user-driven move of the window (call from a
    /// mousedown handler on a drag region)
    fn drag_window(&self) {}

    /// Open developer tools inspecting the document with the given id
    fn open_devtools(&self, doc_id: usize) {
        let _ = doc_id;
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
    /// An exact viewport size in CSS pixels, which takes the place of
    /// `window_size / scale` when set. A window has a whole number of
    /// physical pixels, but a paged medium does not: the page area of an A4
    /// page with 18mm margins is 657.6px wide, and a browser lays it out at
    /// that width. See [`set_logical_size`](Self::set_logical_size).
    pub logical_size_override: Option<(f32, f32)>,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            window_size: (0, 0),
            hidpi_scale: 1.0,
            zoom: 1.0,
            color_scheme: ColorScheme::Light,
            logical_size_override: None,
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
            logical_size_override: None,
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

    /// The viewport size in CSS pixels: the exact size set with
    /// [`set_logical_size`](Self::set_logical_size), or else the physical
    /// window size divided by the total scale factor
    pub fn logical_size(&self) -> (f32, f32) {
        if let Some(size) = self.logical_size_override {
            return size;
        }
        let scale = self.scale();
        (
            self.window_size.0 as f32 / scale,
            self.window_size.1 as f32 / scale,
        )
    }

    /// Set an exact viewport size in CSS pixels, which may be fractional
    /// (a page area when printing), and a matching `window_size` (rounded
    /// up to whole physical pixels). Layout, media queries and viewport
    /// units then use the exact size.
    ///
    /// Assigning `window_size` directly afterwards does not clear it; call
    /// [`clear_logical_size`](Self::clear_logical_size) for that.
    pub fn set_logical_size(&mut self, width: f32, height: f32) {
        let scale = self.scale();
        self.window_size = (
            (width * scale).ceil() as u32,
            (height * scale).ceil() as u32,
        );
        self.logical_size_override = Some((width, height));
    }

    /// Size the viewport from `window_size` again, dropping an exact size
    /// set with [`set_logical_size`](Self::set_logical_size)
    pub fn clear_logical_size(&mut self) {
        self.logical_size_override = None;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_size_keeps_a_fractional_size() {
        let mut viewport = Viewport::new(0, 0, 2.0, ColorScheme::Light);
        viewport.set_logical_size(657.6, 986.5);
        assert_eq!(viewport.logical_size(), (657.6, 986.5));
        assert_eq!(viewport.window_size, (1316, 1973));

        viewport.clear_logical_size();
        assert_eq!(viewport.logical_size(), (658.0, 986.5));
    }
}
