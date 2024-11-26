use blitz_shell::Config;

fn main() {
    blitz_shell::launch_static_html_cfg(
        include_str!("./assets/gosub_reduced.html"),
        Config {
            stylesheets: Vec::new(),
            base_url: Some(String::from("https://gosub.io/")),
        },
    );
}
