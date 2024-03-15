use std::f32::consts::PI;

use dioxus::prelude::*;
use keyboard_types::Modifiers;

#[tokio::main]
async fn main() {
    blitz::launch(app).await;
}

fn app(cx: Scope) -> Element {
    let mut count = use_state(cx, || 0);

    use_future(cx, (), move |_| {
        let count = count.to_owned();
        let update = cx.schedule_update();
        async move {
            loop {
                count.with_mut(|f| *f += 1);
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                update();
            }
        }
    });

    let top_left = (1.0 + (*count.get() as f32 / 100.0).sin()) * 25.0;
    let top_right = (1.0 + (PI * 0.5 + *count.get() as f32 / 100.0).sin()) * 25.0;
    let bottom_right = (1.0 + (PI * 1.0 + *count.get() as f32 / 100.0).sin()) * 25.0;
    let bottom_left = (1.0 + (PI * 1.5 + *count.get() as f32 / 100.0).sin()) * 25.0;

    let smooth = ((std::f32::consts::TAU * (*count.get() % 200) as f32 / 200.0).sin() + 1.0) / 2.0;
    let color = (smooth * 255.0) as i32;
    let width = (smooth * 30.0) as i32;
    let smooth_offset =
        ((std::f32::consts::TAU * ((*count.get() + 100) % 200) as f32 / 200.0).sin() + 1.0) / 2.0;
    let color_offset = (smooth_offset * 255.0) as i32;
    let width_offset = (smooth_offset * 30.0) as i32;

    cx.render(rsx! {
        div {
            width: "100%",
            height: "100%",
            background_color: "rgb(75%, 75%, 75%)",
            onkeydown: |e| {
                if e.data.modifiers().contains(Modifiers::SHIFT) {
                    count.with_mut(|f| *f -= 10);
                } else {
                    count.with_mut(|f| *f += 10);
                }
            },

            div {
                width: "50%",
                height: "100%",
                background_color: "hsl({color}, 100%, 50%)",
                justify_content: "center",
                align_items: "center",
                border_top_left_radius: "{top_left}%",
                border_top_right_radius: "{top_right}%",
                border_bottom_right_radius: "{bottom_right}%",
                border_bottom_left_radius: "{bottom_left}%",
                border_style: "solid",
                border_color: "hsl({color_offset}, 100%, 50%)",
                border_width: "{width}px",
                color: "red",

                onmouseenter: move |_| {
                    count += 10;
                },

                "Hello left {count}!"
            }

            div {
                width: "50%",
                height: "100%",
                background_color: "hsl({color_offset}, 100%, 50%)",
                justify_content: "center",
                align_items: "center",
                border_top_right_radius: "{top_left}%",
                border_top_left_radius: "{top_right}%",
                border_bottom_left_radius: "{bottom_right}%",
                border_bottom_right_radius: "{bottom_left}%",
                border_style: "solid",
                border_color: "hsl({color}, 100%, 50%)",
                border_width: "{width_offset}px",
                color: "blue",

                onmouseenter: move |_| {
                    count -= 10;
                },
                onclick: move |_| {
                    count += 100;
                },

                "Hello right {count}!"
            }
        }
    })
}
