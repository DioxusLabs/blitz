#[cfg(any(target_os = "android", target_arch = "wasm32"))]
mod app;

#[cfg(target_arch = "wasm32")]
mod wasm;

/// Run with `cargo apk run`
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(android_app: dioxus_native::AndroidApp) {
    dioxus_native::set_android_app(android_app);
    dioxus_native::launch(app::app)
}
