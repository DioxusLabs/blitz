// Example: scrolling.
// Creates a scrollable element to demo being able to scroll elements when their content size
// exceeds their layout size
use dioxus::prelude::*;

fn root() -> Element {
    let css = r#"
        .scrollable {
            background-color: green;
            overflow: scroll;
            height: 200px;
        }

        .gap {
            height: 300px;
            margin: 8px;
            background: #11ff11;
            display: flex;
            align-items: center;
            color: white;
        }

        .not-scrollable {
            background-color: yellow;
            padding-top: 16px;
            padding-bottom: 16px;
        "#;

    rsx! {
        style { {css} }
        div { class: "not-scrollable", "Not scrollable" }
        div { class: "scrollable",
            div {
                "Scroll me"
            }
            div {
                class: "gap",
                "gap"
            }
            div {
                "Hello"
            }
        }
        div { class: "not-scrollable", "Not scrollable" }
    }
}

fn main() {
    dioxus_blitz::launch(root);
}
