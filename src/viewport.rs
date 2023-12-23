use tao::dpi::PhysicalSize;

#[derive(Default)]
pub struct Viewport {
    pub window_size: PhysicalSize<u32>,

    hidpi_scale: f32,

    zoom: f32,

    pub font_size: f32,
}

impl Viewport {
    pub fn new(window_size: PhysicalSize<u32>) -> Self {
        Self {
            window_size,
            hidpi_scale: 1.0,
            zoom: 3.0,
            font_size: 32.0,
        }
    }

    // Total scaling, the product of the zoom and hdpi scale
    pub fn scale(&self) -> f32 {
        self.hidpi_scale * self.zoom
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

    pub fn zoom_mut(&mut self) -> &mut f32 {
        &mut self.zoom
    }
}
