use dioxus::{events::KeyCode, prelude::*};

fn main() {
    blitz::launch(app);
}

#[derive(PartialEq, Props)]
struct ButtonProps {
    color_offset: u32,
}

#[allow(non_snake_case)]
fn Button(cx: Scope<ButtonProps>) -> Element {
    let toggle = use_state(&cx, || false);

    let count = cx.props.color_offset % 255;
    let color = if *toggle.get() {
        format!("hsl({count}, 75%, 50%)")
    } else {
        format!("hsl({count}, 25%, 50%)")
    };

    cx.render(rsx! {
        div{
            margin: "1px",
            width: "100%",
            height: "100%",
            background_color: "{color}",
            prevent_default: "false",
            onkeydown: |e| {
                if let KeyCode::Space = e.data.key_code{
                    toggle.modify(|f| !f);
                }
            },
        }
    })
}

fn app(cx: Scope) -> Element {
    cx.render(rsx! {
        div {
            display: "flex",
            flex_direction: "column",
            width: "100%",
            height: "100%",

            (1..20).map(|y|
                cx.render(rsx!{
                    div{
                        display: "flex",
                        flex_direction: "row",
                        width: "100%",
                        height: "100%",
                        (1..20).map(|x|{
                            if (x + y) % 2 == 0{
                                cx.render(rsx!{
                                    div{
                                        width: "100%",
                                        height: "100%",
                                        background_color: "rgb(100, 100, 100)",
                                    }
                                })
                            }
                            else{
                                cx.render(rsx!{
                                    Button{
                                        color_offset: x * y
                                    }
                                })
                            }
                        })
                    }
                })
            )
        }
    })
}
