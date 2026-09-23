use dioxus_native::prelude::*;

fn main() {
    dioxus_native::launch(app);
}

fn app() -> Element {
    let mut drag_active = use_signal(|| false);
    let mut files = use_store(Vec::new);
    let mut texts = use_store(Vec::new);

    rsx! {
        div {
            width: "200px",
            height: "50px",
            border: "1px solid black",
            draggable: "true",
            ondragstart: move |e| {
                println!("dragstart");
                if let Err(err) = e.data_transfer().set_data("text/plain", "Some text") {
                    dbg!(err);
                }
            },
            ondrag: move |_| {
                //println!("drag");
            },
            ondragend: move |_| {
                println!("dragend");
            },
            "Drag me!"
        }
        div {
            width: "400px",
            height: "400px",
            border: if drag_active() { "2px solid black" } else {
                "2px solid gray"
            },
            ondragenter: move |_| {
                drag_active.set(true);
                println!("enter");
            },
            ondragover: move |evt| {
                evt.prevent_default();
                //println!("over");
            },
            ondragleave: move |_| {
                drag_active.set(false);
                println!("leave");
            },
            ondrop: move |evt| {
                evt.prevent_default();
                drag_active.set(false);
                println!("drop");
                for file in evt.data_transfer().files() {
                    files.push( file.name());
                }
                if let Some(text) = evt.data_transfer().get_as_text() {
                    texts.push(text);
                }
                if let Some(text) = evt.data_transfer().get_data("text/html") {
                    texts.push(text);
                }
                if let Some(text) = evt.data_transfer().get_data("text/rtf") {
                    texts.push(text);
                }
            },
            "Files"
            ul {
                for name in files.iter() {
                    li {{name}}
                }
            }
            "Text"
            ul {
                for text in texts.iter() {
                    li {{text}}
                }
            }
            DragArea{
                width: "200px",
                height: "200px",
                padding: "4px",
                color: "green",
                DragArea{
                    width: "100px",
                    height: "100px",
                    color: "blue"
                }
                DragArea{
                    width: "100px",
                    height: "100px",
                    color: "red"
                }
            }
        }
    }
}

#[component]
fn DragArea(
    color: String,
    #[props(extends=GlobalAttributes)] attributes: Vec<Attribute>,
    children: Element,
) -> Element {
    let mut drag_active = use_signal(|| false);
    let color = use_signal(|| color);

    rsx!(div {
        border: if drag_active() {
            "2px solid black"
        } else { "2px solid {color}"},
        ondragenter: move |evt| {
            evt.stop_propagation();
            drag_active.set(true);
            println!("enter {}", color);
        },
        ondragover: move |evt| {
            evt.prevent_default();
            //println!("over {}", color);
        },
        ondragleave: move |evt| {
            evt.stop_propagation();
            drag_active.set(false);
            println!("leave {}", color);
        },
        ondrop: move |_| {
            drag_active.set(false);
            println!("drop {}", color);
        },
        ..attributes,
        {children}
    })
}
