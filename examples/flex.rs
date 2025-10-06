/*
Servo doesn't have:
- space-evenly?
- gap
*/

use dioxus::prelude::*;

fn main() {
    mini_dxn::launch(app);
}

fn app() -> Element {
    rsx! {
        div {
            style { {CSS} }
            div {
                h2 { "justify-content" }
                for row in ["flex-start", "flex-end", "center", "space-between", "space-around", "space-evenly"] {
                    h3 { "{row}" }
                    div { id: "container", justify_content: "{row}",
                        div { class: "floater", "__1__" }
                        div { class: "floater", "__2__" }
                        div { class: "floater", "__3__" }
                    }
                }
            }
            h3 { "CSS Grid Test"}
            div {
                id: "grid_container",
                for _ in 0..3 {
                    div { class: "floater", "__1__" }
                    div { class: "floater", "__2__" }
                    div { class: "floater", "__3__" }
                    div { class: "floater", "__4__" }
                    div { class: "floater", "__5__" }
                    div { class: "floater", "__6__" }
                    div { class: "floater", "__7__" }
                    div { class: "floater", "__8__" }
                    div { class: "floater", "__9__" }
                    div { class: "floater", "__0__" }
                    div { class: "floater", "__A__" }
                    div { class: "floater", "__B__" }
                }
            }
        }
    }
}

const CSS: &str = r#"
#container {
    flex: 1 1 auto;
    flex-direction: row;
    background-color: gray;
    border: 1px solid black;
    border-top-color: red;
    border-left: 4px solid #000;
    border-top: 10px solid #ff0;
    border-right:  3px solid #F01;
    border-bottom:  9px solid #0f0;
    box-shadow: 10px 10px gray;


    outline-style: solid;
    outline-color: blue;
    border-radius: 50px 20px;
    padding: 10px;
    margin: 5px;
    display: flex;
    gap: 10px;
}

div {
    font-family: sans-serif;
}

h3 {
    font-size: 2em;
}

#grid_container {
  display: grid;
  grid-template-columns: 100px 1fr 1fr 100px;
  gap: 10px;
  padding: 10px;
}

.floater {
    background-color: orange;
    border: 3px solid black;
    padding: 10px;
    border-radius: 5px;
    // margin: 0px 10px 0px 10px;
}
"#;
