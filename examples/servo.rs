use dioxus_native::Config;

fn main() {
    dioxus_native::launch_static_html_cfg(
        include_str!("./assets/servo.html"),
        Config {
            stylesheets: Vec::new(),
            base_url: Some(String::from("https://servo.org/")),
        },
    );
}
