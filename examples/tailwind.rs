//! Drive the renderer from Dioxus
//!
//!
//!
//!

use dioxus::prelude::*;

fn main() {
    dioxus_blitz::launch(app);
}

fn app() -> Element {
    rsx! {
        style { {CSS} }
        for _row in 0..3 {
                div { class: "flex flex-row",
                div { id: "cool", "h123456789asdjkahskj\nhiiiii" }
                p { class: "cool", "hi" }
                for x in 1..=9 {
                    div { class: "bg-red-{x}00 border", "{x}" }
                }
            }
        }
    }
}

const CSS: &str = r#"
p.cool { background-color: purple; }
#cool {
    background-color: blue;
    font-size: 32px;
    box-shadow: 16px 16px 16px black;
}
.bg-red-100	{ background-color: rgb(254 226 226); }
.bg-red-200	{ background-color: rgb(254 202 202); }
.bg-red-300	{ background-color: rgb(252 165 165); }
.bg-red-400	{ background-color: rgb(248 113 113); }
.bg-red-500	{ background-color: rgb(239 68 68); }
.bg-red-600	{ background-color: rgb(220 38 38); }
.bg-red-700	{ background-color: rgb(185 28 28); }
.bg-red-800	{ background-color: rgb(153 27 27); }
.bg-red-900	{ background-color: rgb(127 29 29); }
.bg-red-950	{ background-color: rgb(69 10 10); }
.border {
    border: 1rem solid;
}
"#;
