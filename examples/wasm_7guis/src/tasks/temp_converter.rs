use dioxus_native::prelude::*;

#[component]
pub fn TempConverter() -> Element {
    let mut celsius = use_signal(|| String::from("0"));
    let mut fahrenheit = use_signal(|| String::from("32"));

    rsx! {
        div { class: "temp-root",
            style { {CSS} }
            div { class: "temp-card",
                input {
                    class: "temp-input",
                    value: "{celsius}",
                    oninput: move |evt| {
                        let s = evt.value();
                        if let Ok(c) = s.parse::<f64>() {
                            fahrenheit.set(format!("{:.2}", c * 9.0 / 5.0 + 32.0));
                        }
                        celsius.set(s);
                    }
                }
                span { class: "temp-label", " °C = " }
                input {
                    class: "temp-input",
                    value: "{fahrenheit}",
                    oninput: move |evt| {
                        let s = evt.value();
                        if let Ok(f) = s.parse::<f64>() {
                            celsius.set(format!("{:.2}", (f - 32.0) * 5.0 / 9.0));
                        }
                        fahrenheit.set(s);
                    }
                }
                span { class: "temp-label", " °F" }
            }
        }
    }
}

const CSS: &str = r#"
.temp-root {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    height: 100%;
    background-color: #f5f5f5;
    font-family: sans-serif;
}

.temp-card {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 8px;
    background: #ffffff;
    border: 1px solid #d0d0d0;
    border-radius: 8px;
    padding: 24px 32px;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.08);
}

.temp-input {
    font-size: 16px;
    color: #1a1a1a;
    background-color: #fafafa;
    border: 1px solid #c0c0c0;
    border-radius: 4px;
    padding: 8px 12px;
    width: 100px;
    text-align: right;
}

.temp-input:focus {
    outline: none;
    border-color: #4a6cf7;
    box-shadow: 0 0 0 2px rgba(74, 108, 247, 0.15);
}

.temp-label {
    font-size: 16px;
    font-weight: 500;
    color: #444444;
    white-space: nowrap;
}
"#;
