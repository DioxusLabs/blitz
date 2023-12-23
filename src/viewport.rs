use tao::dpi::PhysicalSize;

#[derive(Default)]
pub struct Viewport {
    pub window_size: PhysicalSize<u32>,

    pub hidpi_scale: f32,

    pub zoom: f32,

    pub font_size: f32,
}

impl Viewport {
    pub fn new(window_size: PhysicalSize<u32>) -> Self {
        Self {
            window_size,
            hidpi_scale: 1.0,
            zoom: 1.0,
            font_size: 32.0,
        }
    }

    // Total scaling, the product of the zoom and hdpi scale
    pub fn scale(&self) -> f32 {
        self.hidpi_scale * self.zoom
    }
}
