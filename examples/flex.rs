/*
Servo doesn't have:
- space-evenly?
- gap
*/

use dioxus::prelude::*;

fn main() {
    dioxus_blitz::launch(app);
}

fn app(cx: Scope) -> Element {
    render! {
        style { CSS }
        div {
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
            h3 { "flex-flow" }
            div {
                id: "grid_container",
                div { class: "floater", "__1__" }
                div { class: "floater", "__2__" }
                div { class: "floater", "__3__" }
                div { class: "floater", "__4__" }
                div { class: "floater", "__5__" }
                div { class: "floater", "__6__" }
                div { class: "floater", "__7__" }
                div { class: "floater", "__8__" }
                div { class: "floater", "__9__" }
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
#grid_container {
  /* We first create a flex layout context */
  display: flex;

  /* Then we define the flow direction
     and if we allow the items to wrap
   * Remember this is the same as:
   * flex-direction: row;
   * flex-wrap: wrap;
   */
  flex-flow: row wrap;

  /* Then we define how is distributed the remaining space */
  justify-content: space-around;
}

.floater {
    background-color: orange;
    border: 3px solid black;
    padding: 10px;
    border-radius: 5px;
    // margin: 0px 10px 0px 10px;
}
"#;
