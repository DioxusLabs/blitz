use dioxus::prelude::*;

fn main() {
    dioxus_blitz::launch(app);
}

fn app(cx: Scope) -> Element {
    render! {
        style { CSS }
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
