use crate::document::DummyFontMetricsProvider;
use style::{
    media_queries::{Device, MediaType},
    properties::{style_structs::Font, ComputedValues},
};

/// Window viewport.
#[derive(Default, Debug, Clone)]
pub struct Viewport {
    /// Size of the window.
    pub window_size: (u32, u32),

    /// Font size.
    pub font_size: f32,

    /// Zoom level.
    pub zoom: f32,

    hidpi_scale: f32,
}

impl Viewport {
    /// Create a new viewport from a window's physical size and scale factor.
    pub fn new(physical_width: u32, physical_height: u32, scale_factor: f32) -> Self {
        Self {
            window_size: (physical_width, physical_height),
            hidpi_scale: scale_factor,
            zoom: 1.0,
            font_size: 16.0,
        }
    }

    /// Total scaling, the product of the zoom and hdpi scale.
    pub fn scale(&self) -> f32 {
        self.hidpi_scale * self.zoom
    }

    /// Total scaling, the product of the zoom and hdpi scale (as an `f64`).
    pub fn scale_f64(&self) -> f64 {
        self.scale() as f64
    }

    /// Set the hidi scale.
    pub fn set_hidpi_scale(&mut self, scale: f32) {
        self.hidpi_scale = scale;
    }

    pub(crate) fn make_device(&self) -> Device {
        let width = self.window_size.0 as f32 / self.scale();
        let height = self.window_size.1 as f32 / self.scale();
        let viewport_size = euclid::Size2D::new(width, height);
        let device_pixel_ratio = euclid::Scale::new(self.scale());

        Device::new(
            MediaType::screen(),
            selectors::matching::QuirksMode::NoQuirks,
            viewport_size,
            device_pixel_ratio,
            Box::new(DummyFontMetricsProvider),
            ComputedValues::initial_values_with_font_override(Font::initial_values()),
        )
    }
}
