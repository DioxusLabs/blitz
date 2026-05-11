// On Windows do NOT show a console window when opening the app
#![cfg_attr(all(not(test), target_os = "windows"), windows_subsystem = "windows")]

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    dioxus_native::launch(seven_guis::app::app);
}

#[cfg(target_arch = "wasm32")]
fn main() {}
