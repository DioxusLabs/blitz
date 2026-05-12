pub mod app;
pub mod tasks;

#[cfg(target_arch = "wasm32")]
mod wasm_entry {
    use dioxus_native::{Config, build_single_font_ctx};
    use wasm_bindgen::prelude::*;

    const DEJAVU_SANS: &[u8] = include_bytes!("../assets/DejaVuSans.woff2");

    #[wasm_bindgen(start)]
    pub fn start() {
        console_error_panic_hook::set_once();
        let font_ctx = build_single_font_ctx(DEJAVU_SANS);
        dioxus_native::launch_cfg(
            super::app::app,
            vec![],
            vec![Box::new(Config::new().with_font_ctx(font_ctx))],
        );
    }
}
