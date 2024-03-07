use dioxus::prelude::*;
use std::thread::Scope;

fn main() {
    dioxus_blitz::launch(app);
}

fn app() -> Element {
    rsx! {
        style { {CSS} }
        div { id: "a",
            "Some text"
            em { "Another block of text" }
            "Should connect no space between"
        }
    }
}

const CSS: &str = r#"
#a {
}
"#;
