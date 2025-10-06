use dioxus::prelude::*;

fn main() {
    mini_dxn::launch(app);
}

fn app() -> Element {
    let mut box1_hover = use_signal(|| 0);
    let mut box2_hover = use_signal(|| 0);
    let mut box3_hover = use_signal(|| 0);

    rsx! {
        style { {STYLES} }
        div { class: "container",
            // Box 1 - Simple hover counter
            div { class: "hover-box", onmouseover: move |_| box1_hover += 1,
                "Hover Count: {box1_hover}"
            }

            // Box 2 - Parent with child
            div {
                class: "hover-box parent",
                onmouseover: move |_| box2_hover += 1,
                "Parent Hovers: {box2_hover}"
                div { class: "child", onmouseover: move |_| box3_hover += 1,
                    "Child Hovers: {box3_hover}"
                }
            }
        }
    }
}

static STYLES: &str = r#"
    .container {
        display: flex;
        gap: 20px;
        padding: 20px;
    }

    .hover-box {
        padding: 20px;
        background: #eee;
        border: 2px solid #333;
        border-radius: 8px;
        cursor: pointer;
        transition: all 0.2s;
    }

    .hover-box:hover {
        background: #333;
        color: white;
        transform: scale(1.05);
    }

    .parent {
        position: relative;
        min-width: 200px;
    }

    .child {
        margin-top: 10px;
        padding: 10px;
        background: #666;
        color: white;
        border-radius: 4px;
    }

    .child:hover {
        background: #999;
    }

    .stats {
        position: fixed;
        top: 20px;
        right: 20px;
        padding: 10px;
        background: #333;
        color: white;
        border-radius: 4px;
    }
"#;
