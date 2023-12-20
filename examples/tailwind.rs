//! Drive the renderer from Dioxus
//!
//!
//!
//!

use dioxus::prelude::*;
use stylo_dioxus::Config;

fn main() {
    let cfg = Config {
        stylesheets: vec![CSS.to_string()],
    };
    stylo_dioxus::launch_cfg(app, cfg);
}

fn app(cx: Scope) -> Element {
    render! {
        for _ in 0..5 {
            div { class: "bg-red-100", "1" }
            div { class: "bg-red-200", "2" }
            div { class: "bg-red-300", "3" }
            div { class: "bg-red-400", "4" }
            div { class: "bg-red-500", "5" }
            div { class: "bg-red-600", "6" }
            div { class: "bg-red-700", "7" }
            div { class: "bg-red-800", "8" }
            div { class: "bg-red-900", "9" }
            div { class: "bg-red-950", "10" }
        }
    }
}

const CSS: &str = r#"
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
"#;
