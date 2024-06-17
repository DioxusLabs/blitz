use style::{
    media_queries::{Device, MediaType},
    servo::media_queries::FontMetricsProvider,
};

#[derive(Default, Debug)]
pub struct Viewport {
    pub window_size: (u32, u32),

    hidpi_scale: f32,

    zoom: f32,

    pub font_size: f32,
}

// TODO: implement a proper font metrics provider
#[derive(Debug, Clone)]
struct DummyFontMetricsProvider;
impl FontMetricsProvider for DummyFontMetricsProvider {
    fn query_font_metrics(
        &self,
        _vertical: bool,
        _font: &style::properties::style_structs::Font,
        _base_size: style::values::computed::CSSPixelLength,
        _in_media_query: bool,
        _retrieve_math_scales: bool,
    ) -> style::font_metrics::FontMetrics {
        Default::default()
    }
}

impl Viewport {
    pub fn new(window_size: (u32, u32)) -> Self {
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
        )
    }
}
