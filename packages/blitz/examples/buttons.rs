use dioxus::prelude::*;

#[tokio::main]
async fn main() {
    blitz::launch(app).await;
}

#[derive(PartialEq, Props, Clone)]
struct ButtonProps {
    color_offset: u32,
    layer: u16,
}

#[allow(non_snake_case)]
fn Button(props: ButtonProps) -> Element {
    let mut toggle = use_signal(|| false);
    let mut hovered = use_signal(|| false);

    let hue = props.color_offset % 255;
    let saturation = if toggle() { 50 } else { 25 } + if hovered() { 50 } else { 25 };
    let brightness = saturation / 2;
    let color = format!("hsl({hue}, {saturation}%, {brightness}%)");

    rsx! {
        div {
            border_width: "0px",
            width: "100%",
            height: "100%",
            background_color: "{color}",
            tabindex: "{props.layer}",
            onkeydown: move |e| {
                if e.code() == keyboard_types::Code::Space {
                    toggle.toggle();
                }
            },
            onmouseup: move |_| {
                toggle.toggle();
            },
            onmouseenter: move |_| {
                hovered.set(true);
            },
            onmouseleave: move |_| {
                hovered.set(false);
            },
            justify_content: "center",
            align_items: "center",
            text_align: "center",
            display: "flex",
            flex_direction: "column",

            p { "{props.layer}" }
        }
    }
}

fn app(_: ()) -> Element {
    let mut count = use_signal(|| 10);
    let current_count = count();
    rsx! {
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
                onmouseup: move |_| {
                    count += 10;
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
                for y in (0..current_count) {

                    div { display: "flex", flex_direction: "row", width: "100%", height: "100%",
                        for x in (0..current_count) {
                            if (x + y) % 2 == 0 {
                                div {
                                    border_width: "0px",
                                    width: "100%",
                                    height: "100%",
                                    background_color: "rgb(100, 100, 100)"
                                }
                            } else {
                                Button {
                                    color_offset: x * y,
                                    layer: ((x + y) % 3) as u16
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
