// On Windows do NOT show a console window when opening the app
#![cfg_attr(all(not(test), target_os = "windows"), windows_subsystem = "windows")]

//! The typical TodoMVC app, implemented in Dioxus.

mod app;

fn main() {
    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt::init();
    dioxus_native::launch(app::app)
}
