// On Windows do NOT show a console window when opening the app
#![cfg_attr(all(not(test), target_os = "windows"), windows_subsystem = "windows")]

//! An example of a Dioxus Native app with a transparent window background.
//!
//! Transparency requires three things to line up:
//!   1. The winit window itself must be created as transparent
//!      (`WindowAttributes::with_transparent(true)`).
//!   2. The renderer must composite the surface using an alpha-aware mode and
//!      clear each frame to a transparent base color (via the dioxus-native
//!      [`Config`]).
//!   3. The page's CSS must not paint an opaque background, otherwise the HTML
//!      content would cover up the transparent window.

mod app;

#[unsafe(no_mangle)]
#[cfg(target_os = "android")]
pub fn android_main(android_app: dioxus_native::AndroidApp) {
    dioxus_native::set_android_app(android_app);
    app::launch();
}

fn main() {
    app::launch();
}
