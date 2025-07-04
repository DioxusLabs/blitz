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

#[derive(Default, Debug, Clone, Copy)]
pub enum ColorScheme {
    #[default]
    Light,
    Dark,
}

#[derive(Default, Debug, Clone)]
pub struct Viewport {
    pub window_size: (u32, u32),

    hidpi_scale: f32,

    zoom: f32,

    pub font_size: f32,

    pub color_scheme: ColorScheme,
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
            font_size: 16.0,
            color_scheme,
        }
    }

    // Total scaling, the product of the zoom and hdpi scale
    pub fn scale(&self) -> f32 {
        self.hidpi_scale * self.zoom
    }
    // Total scaling, the product of the zoom and hdpi scale
    pub fn scale_f64(&self) -> f64 {
        self.scale() as f64
    }

    pub fn set_hidpi_scale(&mut self, scale: f32) {
        self.hidpi_scale = scale;
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

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
