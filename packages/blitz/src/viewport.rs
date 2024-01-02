use style::media_queries::{Device, MediaType};
use tao::dpi::PhysicalSize;

#[derive(Default, Debug)]
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
            zoom: 1.0,
            font_size: 16.0,
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

    pub fn zoom_mut(&mut self) -> &mut f32 {
        &mut self.zoom
    }

    pub fn make_device(&self) -> Device {
        let height = self.window_size.height as f32 / self.scale();
        let width = self.window_size.width as f32 / self.scale();
        let viewport_size = euclid::Size2D::new(width, height);
        let device_pixel_ratio = euclid::Scale::new(self.scale());

        Device::new(
            MediaType::screen(),
            selectors::matching::QuirksMode::NoQuirks,
            viewport_size,
            device_pixel_ratio,
        )
    }
}
