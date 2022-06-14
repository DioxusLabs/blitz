use std::f32::consts::PI;

use dioxus::prelude::*;

fn main() {
    blitz::launch(app);
}

fn app(cx: Scope) -> Element {
    let count = use_state(&cx, || 0);

    use_future(&cx, (), move |_| {
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

    cx.render(rsx! {
        div {
            width: "100%",

            div {
                width: "50%",
                height: "100%",
                background_color: "blue",
                justify_content: "center",
                align_items: "center",
                border_top_left_radius: "{top_left}%",
                border_top_right_radius: "{top_right}%",
                border_bottom_right_radius: "{bottom_right}%",
                border_bottom_left_radius: "{bottom_left}%",
                border_style: "solid",
                border_color: "red",
                border_width: "5px",
                color: "red",

                "Hello left {count}!"
            }
            div {
                width: "50%",
                height: "100%",
                background_color: "red",
                justify_content: "center",
                align_items: "center",
                border_top_right_radius: "{top_left}%",
                border_top_left_radius: "{top_right}%",
                border_bottom_left_radius: "{bottom_right}%",
                border_bottom_right_radius: "{bottom_left}%",
                border_style: "solid",
                border_color: "blue",
                border_width: "5px",
                color: "blue",

                "Hello right {count}!"
            }
        }
    })
}
