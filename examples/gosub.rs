use dioxus_blitz::Config;

fn main() {
    dioxus_blitz::launch_static_html_cfg(
        include_str!("./assets/gosub_reduced.html"),
        Config {
            stylesheets: Vec::new(),
            base_url: Some(String::from("https://gosub.io/")),
        },
    );
}
