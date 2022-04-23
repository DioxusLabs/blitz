use dioxus::{events::KeyCode, prelude::*};

fn main() {
    blitz::launch(app);
}

#[derive(PartialEq, Props)]
struct ButtonProps {
    color_offset: u32,
    layer: u16,
}

#[allow(non_snake_case)]
fn Button(cx: Scope<ButtonProps>) -> Element {
    let toggle = use_state(&cx, || false);

    let hue = cx.props.color_offset % 255;
    let color = if *toggle.get() {
        format!("hsl({hue}, 75%, 50%)")
    } else {
        format!("hsl({hue}, 25%, 50%)")
    };

    cx.render(rsx! {
        div{
            margin: "1px",
            width: "100%",
            height: "100%",
            background_color: "{color}",
            // prevent_default: "false",
            tabindex: "{cx.props.layer}",
            onkeydown: |e| {
                if let KeyCode::Space = e.data.key_code{
                    toggle.modify(|f| !f);
                }
            },
            justify_content: "center",
            align_items: "center",
            text_align: "center",

            "tabindex: {cx.props.layer}"
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

            (1..8).map(|y|
                cx.render(rsx!{
                    div{
                        display: "flex",
                        flex_direction: "row",
                        width: "100%",
                        height: "100%",
                        (1..8).map(|x|{
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
                                let layer = (x + y) % 3;
                                cx.render(rsx!{
                                    Button{
                                        color_offset: x * y,
                                        layer: layer as u16,
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
