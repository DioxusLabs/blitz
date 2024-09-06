use dioxus::prelude::*;

fn main() {
    tracing_subscriber::fmt::init();

    dioxus_blitz::launch(app);
}

fn app() -> Element {
    rsx! {
        body {
            style { {CSS} }
            div { id: "a",
                "Some text"
                em { "Another block of text" }
                "Should connect no space between"
            }
            ol {
                li { "Item 1 " }
                li { "Item 2" }
            }
        }
    }
}

const CSS: &str = r#"
#a {
}
"#;
