//! Drive the renderer from Dioxus
use dioxus_native::prelude::*;

pub fn app() -> Element {
    let mut count = use_signal(|| 0);

    rsx! {
        div { class: "container",
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

html, body, #main {
    padding: 0;
    margin: 0;
    background-color: green;
    height: 100%;
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
    height: 100%;
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
    border-width: 2px;
    border-style: solid;
}
.counter-button:focus {
    outline: 4px solid black;
}

.btn-green {
    background-color: green;
    border-color: green;
    color: white;
}
.btn-green:hover {
    color: green;
    background-color: white;
}

.btn-red {
    background-color: red;
    border-color: red;
    color: white;
}
.btn-red:hover {
    color: red;
    background-color: white;
}

.btn-blue {
    background-color: blue;
    border-color: blue;
    color: white;
}
.btn-blue:hover {
    color: blue;
    background-color: white;
}


"#;
