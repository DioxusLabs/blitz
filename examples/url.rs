//! Load first CLI argument as a url. Fallback to google.com if no CLI argument is provided.

fn main() {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "https://www.google.com".into());
    blitz::launch_url(&url);
}
