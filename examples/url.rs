//! Load first CLI argument as a url. Fallback to google.com if no CLI argument is provided.

fn main() {
    let url = std::env::args()
        .skip(1)
        .next()
        .unwrap_or_else(|| "https://www.google.com".into());
    dioxus_blitz::launch_url(&url);
}
