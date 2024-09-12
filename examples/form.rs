//! Drive the renderer from Dioxus

use dioxus::prelude::*;

fn main() {
    dioxus_blitz::launch(app);
}

fn app() -> Element {
    let mut checkbox_checked = use_signal(|| false);

    rsx! {
        div {
            class: "container",
            style { {CSS} }
            form {
                div {
                    input {
                        type: "checkbox",
                        id: "check1",
                        name: "check1",
                        value: "check1",
                        checked: checkbox_checked(),
                        // This works too
                        // checked: "{checkbox_checked}",
                        oninput: move |ev| checkbox_checked.set(!ev.checked()),
                    }
                    label {
                        r#for: "check1",
                        "Checkbox 1 (controlled)"
                    }
                }
            div {
                label {
                    input {
                        type: "checkbox",
                        name: "check2",
                        value: "check2",
                    }
                    "Checkbox 2 (uncontrolled)"
                }
            }
        }
            div { "Checkbox 1 checked: {checkbox_checked}" }
        }
    }
}

const CSS: &str = r#"

.container {
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    height: 100vh;
    width: 100vw;
}


form {
    margin: 12px 0;
    display: block;
}

form > div {
    margin: 8px 0;
}

label {
    display: inline-block;
}

input {
    /* Should be accent-color */
    color: #0000cc;
}

"#;
