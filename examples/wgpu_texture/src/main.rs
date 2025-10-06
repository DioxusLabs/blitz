use color::{OpaqueColor, Srgb};
use demo_renderer::{DemoMessage, DemoPaintSource};
use std::env;
use wgpu::{Features, Limits};

mod demo_renderer;
mod dioxus_native;
mod html;

use dioxus_native::launch_dx_native;
use html::launch_html;

// CSS Styles
static STYLES: &str = include_str!("./styles.css");

// WGPU settings required by this example
const FEATURES: Features = Features::PUSH_CONSTANTS;
fn limits() -> Limits {
    Limits {
        max_push_constant_size: 16,
        ..Limits::default()
    }
}

type Color = OpaqueColor<Srgb>;

fn main() {
    let use_html_renderer = env::args().any(|arg| arg == "--html");

    if use_html_renderer {
        // Render WGPU demo using Blitz HTML document
        launch_html();
    } else {
        // Render WGPU demo using dioxus-native
        launch_dx_native();
    }
}
