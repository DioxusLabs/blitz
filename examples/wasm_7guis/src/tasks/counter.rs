use dioxus_native::prelude::*;

#[component]
pub fn Counter() -> Element {
    let mut count = use_signal(|| 0i32);

    rsx! {
        div { class: "counter-root",
            style { {CSS} }
            div { class: "counter-card",
                p { class: "counter-display", "{count}" }
                button {
                    class: "counter-btn",
                    onclick: move |_| { count += 1 },
                    "Count"
                }
            }
        }
    }
}

const CSS: &str = r#"
.counter-root {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    height: 100%;
    background-color: #f5f5f5;
    font-family: sans-serif;
}

.counter-card {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 16px;
    background: #ffffff;
    border: 1px solid #d0d0d0;
    border-radius: 8px;
    padding: 24px 32px;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.08);
}

.counter-display {
    font-size: 24px;
    font-weight: 600;
    color: #1a1a1a;
    min-width: 60px;
    text-align: center;
    margin: 0;
}

.counter-btn {
    font-size: 16px;
    font-weight: 500;
    color: #ffffff;
    background-color: #4a6cf7;
    border: none;
    border-radius: 6px;
    padding: 10px 24px;
    cursor: pointer;
}

.counter-btn:hover {
    background-color: #3a5ce5;
}

.counter-btn:active {
    background-color: #2a4cd3;
}
"#;
