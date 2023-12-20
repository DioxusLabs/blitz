use tao::dpi::PhysicalSize;

#[derive(Default)]
pub struct Viewport {
    pub window_size: PhysicalSize<u32>,
}

impl Viewport {
    pub fn new(window_size: PhysicalSize<u32>) -> Self {
        Self { window_size }
    }
}
