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
        }
    }
}

const CSS: &str = r#"
#a {
}
"#;
