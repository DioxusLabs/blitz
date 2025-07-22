//! Drive the renderer from Dioxus

use dioxus::prelude::*;

fn main() {
    mini_dxn::launch(app);
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
                    input {
                        type: "checkbox",
                        id: "check3",
                        name: "check3",
                        value: "check3",
                    }
                    label {
                        r#for: "check3",
                        "Checkbox 1 (uncontrolled with for)"
                    }
                }
                div {
                    label {
                        input {
                            type: "checkbox",
                            name: "check2",
                            value: "check2",
                        }
                        "Checkbox 2 (uncontrolled nested)"
                    }
                }
                div {
                    label {
                        r#for: "radio1",
                        id: "radio1label",
                        input {
                            type: "radio",
                            name: "radiobuttons",
                            id: "radio1",
                            value: "radiovalue1",
                            checked: true,
                        }
                        "Radio Button 1"
                    }
                }
                div {
                    label {
                        r#for: "radio2",
                        id: "radio2label",
                        input {
                            type: "radio",
                            name: "radiobuttons",
                            id: "radio2",
                            value: "radiovalue2",
                        }
                        "Radio Button 2"
                    }
                }
                div {
                    label {
                        r#for: "radio3",
                        id: "radio3label",
                        input {
                            type: "radio",
                            name: "radiobuttons",
                            id: "radio3",
                            value: "radiovalue3",
                        }
                        "Radio Button 3"
                    }
                }
                div {
                    input {
                        type: "file",
                        name: "single_file",
                        id: "file1",
                    }
                    label {
                        r#for: "file1",
                        "File Select Single",
                    }
                }
                div {
                    input {
                        type: "file",
                        name: "multiple_files",
                        id: "file2",
                        multiple: true,
                    }
                    label {
                        r#for: "file2",
                        "File Select Multiple",
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

input[type=radio]:checked {
    border-color: #0000cc;
}

"#;
