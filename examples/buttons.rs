use dioxus::prelude::*;

#[tokio::main]
async fn main() {
    blitz::launch(app).await;
}

#[derive(PartialEq, Props)]
struct ButtonProps {
    color_offset: u32,
    layer: u16,
}

#[allow(non_snake_case)]
fn Button(cx: Scope<ButtonProps>) -> Element {
    let toggle = use_state(cx, || false);
    let hovered = use_state(cx, || false);

    let hue = cx.props.color_offset % 255;
    let saturation = if *toggle.get() { 50 } else { 25 } + if *hovered.get() { 50 } else { 25 };
    let brightness = saturation / 2;
    let color = format!("hsl({hue}, {saturation}%, {brightness}%)");

    cx.render(rsx! {
        div {
            border_width: "0px",
            width: "100%",
            height: "100%",
            background_color: "{color}",
            tabindex: "{cx.props.layer}",
            onkeydown: |e| {
                if e.code() == keyboard_types::Code::Space {
                    toggle.modify(|f| !f);
                }
            },
            onmouseup: |_| {
                toggle.modify(|f| !f);
            },
            onmouseenter: |_| {
                hovered.set(true);
            },
            onmouseleave: |_| {
                hovered.set(false);
            },
            justify_content: "center",
            align_items: "center",
            text_align: "center",
            display: "flex",
            flex_direction: "column",

            p { "tab index: {cx.props.layer}" }
        }
    })
}

fn app(cx: Scope) -> Element {
    let count = use_state(cx, || 10);
    let current_count = **count;
    cx.render(rsx! {
        div { display: "flex", flex_direction: "column", width: "100%", height: "100%",
            div {
                display: "flex",
                flex_direction: "row",
                justify_content: "center",
                align_items: "center",
                text_align: "center",
                width: "100%",
                height: "10%",
                background_color: "green",
                tabindex: "0",
                onmouseup: |_| {
                    count.modify(|c| *c + 10);
                },
                "grid: {current_count}x{current_count} = {current_count*current_count} tiles - Click to add more"
            }
            div {
                display: "flex",
                flex_direction: "column",
                justify_content: "center",
                align_items: "center",
                text_align: "center",
                width: "100%",
                height: "90%",
                (0..current_count).map(|y|
                    rsx! {
                        div { display: "flex", flex_direction: "row", width: "100%", height: "100%",
                            (0..current_count).map(|x| {
                                if (x + y) % 2 == 0 {
                                    rsx! {
                                        div {
                                            border_width: "0px",
                                            width: "100%",
                                            height: "100%",
                                            background_color: "rgb(100, 100, 100)"
                                        }
                                    }
                                } else {
                                    rsx! {
                                        Button {
                                            color_offset: x * y,
                                            layer: ((x + y) % 3) as u16
                                        }
                                    }
                                }
                            })
                        }
                    }
                )
            }
        }
    })
}
