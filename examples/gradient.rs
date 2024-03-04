use dioxus::prelude::*;

fn main() {
    dioxus_blitz::launch(app);
}

fn app() -> Element {
    rsx! {
        style { {CSS} }
        div {
            // https://developer.mozilla.org/en-US/docs/Web/CSS/gradient/linear-gradient
            class: "flex flex-row",
            div { background: "linear-gradient(#e66465, #9198e5)", id: "a", "Vertical Gradient"}
            div { background: "linear-gradient(0.25turn, #3f87a6, #ebf8e1, #f69d3c)", id: "a", "Horizontal Gradient"}
            div { background: "linear-gradient(to left, #333, #333 50%, #eee 75%, #333 75%)", id: "a", "Multi stop Gradient"}
            div { background: r#"linear-gradient(217deg, rgba(255,0,0,.8), rgba(255,0,0,0) 70.71%),
            linear-gradient(127deg, rgba(0,255,0,.8), rgba(0,255,0,0) 70.71%),
            linear-gradient(336deg, rgba(0,0,255,.8), rgba(0,0,255,0) 70.71%)"#, id: "a", "Complex Gradient"}
        }
        div {
            class: "flex flex-row",
            div { background: "linear-gradient(to right, red 0%, blue 100%)", id: "a", "Unhinted Gradient"}
            div { background: "linear-gradient(to right, red 0%, 0%, blue 100%)", id: "a", "0% Hinted"}
            div { background: "linear-gradient(to right, red 0%, 25%, blue 100%)", id: "a", "25% Hinted"}
            div { background: "linear-gradient(to right, red 0%, 50%, blue 100%)", id: "a", "50% Hinted"}
            div { background: "linear-gradient(to right, red 0%, 100%, blue 100%)", id: "a", "100% Hinted"}
            div { background: "linear-gradient(to right, yellow, red 10%, 10%, blue 100%)", id: "a", "10% Mixed Hinted"}
        }
    }
}

const CSS: &str = r#"
.flex { display: flex; }
.flex-row { flex-direction: row; }
#a {
    height:300px;
    width: 300px;
}
"#;
