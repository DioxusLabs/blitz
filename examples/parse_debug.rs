//! Render minimal html5 page

fn main() {
    dioxus_native::launch_static_html(include_str!("./assets/google_reduced.html"));
}
