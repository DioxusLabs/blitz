#![cfg(target_os = "android")]

mod app;

/// Run with `cargo apk run`
#[unsafe(no_mangle)]
pub fn android_main(android_app: dioxus_native::AndroidApp) {
    dioxus_native::set_android_app(android_app);
    dioxus_native::launch(app::app)
}
