use crate::document::DummyFontMetricsProvider;
use style::{
    media_queries::{Device, MediaType},
    properties::{style_structs::Font, ComputedValues},
    queries::values::PrefersColorScheme,
};

#[derive(Default, Debug, Clone, Copy)]
pub enum ColorScheme {
    #[default]
    Light,
    Dark,
}

impl From<ColorScheme> for PrefersColorScheme {
    fn from(value: ColorScheme) -> Self {
        match value {
            ColorScheme::Light => PrefersColorScheme::Light,
            ColorScheme::Dark => PrefersColorScheme::Dark,
        }
    }
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

    pub fn zoom_mut(&mut self) -> &mut f32 {
        &mut self.zoom
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
            self.color_scheme.into(),
        )
    }
}
