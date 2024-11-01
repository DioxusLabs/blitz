use dioxus_native::Config;
use std::path::Path;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/assets/servo.html");
    let url = format!("file://{}", dir.display());
    dioxus_native::launch_static_html_cfg(
        include_str!("./assets/servo.html"),
        Config {
            stylesheets: Vec::new(),
            base_url: Some(url),
        },
    );
}
