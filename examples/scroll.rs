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
        
        .gap:hover {
            background: red;
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
                onclick: |_| println!("Gap clicked!"),
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
    blitz_shell::launch(root);
}
