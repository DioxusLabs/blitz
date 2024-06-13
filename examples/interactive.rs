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
    let mut count = use_signal(|| 0);

    rsx! {
        div {
            class: "container",
            background: r#"linear-gradient(217deg, rgba(255,0,0,.8), rgba(255,0,0,0) 70.71%),
            linear-gradient(127deg, rgba(0,255,0,.8), rgba(0,255,0,0) 70.71%),
            linear-gradient(336deg, rgba(0,0,255,.8), rgba(0,0,255,0) 70.71%)"#,
            style { {CSS} }
            h1 { class: "header", "Count: {count}" }
            div { class: "buttons",
                button {
                    class: "counter-button btn-green",
                    onclick: move |_| { count += 1 },
                    "Increment"
                }
                button {
                    class: "counter-button btn-red",
                    onclick: move |_| { count -= 1 },
                    "Decrement"
                }
            }
            button {
                class: "counter-button btn-blue",
                onclick: move |_| { count.set(0) },
                "Reset"
            }
        }
    }
}

const CSS: &str = r#"
// h1 {
//     background-color: red;
// }

body {
    line-height: 1;
}

h2 {
    background-color: green;
}

h3 {
    background-color: blue;
}

h4 {
    background-color: yellow;
}

.header {
    background-color: pink;
    padding: 20px;
    line-height: 1;
    font-family: sans-serif;
}

.container {
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    height: 100vh;
    width: 100vw;
    background: linear-gradient(217deg, rgba(255,0,0,.8), rgba(255,0,0,0) 70.71%),
            linear-gradient(127deg, rgba(0,255,0,.8), rgba(0,255,0,0) 70.71%),
            linear-gradient(336deg, rgba(0,0,255,.8), rgba(0,0,255,0) 70.71%);
}

.buttons {
    display: flex;
    flex-direction: row;
    justify-content: center;
    align-items: center;
    margin: 20px 0;
}

.counter-button {
    margin: 0 10px;
    padding: 10px 20px;
    border-radius: 5px;
    font-size: 1.5rem;
    cursor: pointer;
    line-height: 1;
    font-family: sans-serif;
}

.btn-green {
    background-color: green;
    color: white;
}

.btn-red {
    background-color: red;
    color: white;
}

.btn-blue {
    background-color: blue;
    color: white;
}

"#;
