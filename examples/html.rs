use dioxus_blitz::Config;

fn main() {
    let local_file_path = std::env::args()
        .skip(1)
        .next()
        .expect("Path to local HTML should be passed as the first CLI paramater");

    let file_content = std::fs::read_to_string(local_file_path).unwrap();

    dioxus_blitz::launch_static_html_cfg(
        &file_content,
        Config {
            stylesheets: Vec::new(),
            base_url: None,
        },
    );
}
