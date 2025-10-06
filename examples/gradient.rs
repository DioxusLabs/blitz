use dioxus::prelude::*;

fn main() {
    mini_dxn::launch(app);
}

fn app() -> Element {
    rsx! {
        style { {CSS} }
        div {
            class: "grid-container",
            div { id: "a1" }
            div { id: "a2" }
            div { id: "a3" }
            div { id: "a4" }

            div { id: "b1" }
            div { id: "b2" }
            div { id: "b3" }
            div { id: "b4" }
            div { id: "b5" }

            div { id: "c1" }
            div { id: "c2" }
            div { id: "c3" }

            div { id: "d1" }
            div { id: "d2" }
            div { id: "d3" }
            div { id: "d4" }
            div { id: "d5" }

            div { id: "e1" }
            div { id: "e2" }
            div { id: "e3" }
            div { id: "e4" }
            div { id: "e5" }
        }
    }
}

const CSS: &str = r#"
.grid-container {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(100px, 1fr));
    gap: 10px;
    width: 95vw;
    height: 95vh;
}

div {
    min-width: 100px;
    min-height: 100px;
}

#a1 { background: linear-gradient(#e66465, #9198e5) }
#a2 { background: linear-gradient(0.25turn, #3f87a6, #ebf8e1, #f69d3c) }
#a3 { background: linear-gradient(to left, #333, #333 50%, #eee 75%, #333 75%) }
#a4 { background: linear-gradient(217deg, rgba(255,0,0,.8), rgba(255,0,0,0) 70.71%),
    linear-gradient(127deg, rgba(0,255,0,.8), rgba(0,255,0,0) 70.71%),
    linear-gradient(336deg, rgba(0,0,255,.8), rgba(0,0,255,0) 70.71%) }

#b1 { background: linear-gradient(to right, red 0%, 0%, blue 100%) }
#b2 { background: linear-gradient(to right, red 0%, 25%, blue 100%) }
#b3 { background: linear-gradient(to right, red 0%, 50%, blue 100%) }
#b4 { background: linear-gradient(to right, red 0%, 100%, blue 100%) }
#b5 { background: linear-gradient(to right, yellow, red 10%, 10%, blue 100%) }

#c1 { background: repeating-linear-gradient(#e66465, #e66465 20px, #9198e5 20px, #9198e5 25px) }
#c2 { background: repeating-linear-gradient(45deg, #3f87a6, #ebf8e1 15%, #f69d3c 20%) }
#c3 { background: repeating-linear-gradient(transparent, #4d9f0c 40px),
    repeating-linear-gradient(0.25turn, transparent, #3f87a6 20px) }

#d1 { background: radial-gradient(circle, red 20px, black 21px, blue) }
#d2 { background: radial-gradient(closest-side, #3f87a6, #ebf8e1, #f69d3c) }
#d3 { background: radial-gradient(circle at 100%, #333, #333 50%, #eee 75%, #333 75%) }
#d4 { background: radial-gradient(ellipse at top, #e66465, transparent),
    radial-gradient(ellipse at bottom, #4d9f0c, transparent) }
#d5 { background: radial-gradient(closest-corner circle at 20px 30px, red, yellow, green) }
#e1 { background: repeating-conic-gradient(red 0%, yellow 15%, red 33%) }
#e2 { background: repeating-conic-gradient(
    from 45deg at 10% 50%,
    brown 0deg 10deg,
    darkgoldenrod 10deg 20deg,
    chocolate 20deg 30deg
) }
#e3 { background: repeating-radial-gradient(#e66465, #9198e5 20%) }
#e4 { background: repeating-radial-gradient(closest-side, #3f87a6, #ebf8e1, #f69d3c) }
#e5 { background: repeating-radial-gradient(circle at 100%, #333, #333 10px, #eee 10px, #eee 20px) }
"#;
