use dioxus_native::prelude::*;
use std::rc::Rc;

#[derive(Clone, PartialEq)]
struct Circle {
    x: f64,
    y: f64,
    r: f64,
}

#[component]
pub fn CircleDrawer() -> Element {
    let mut circles: Signal<Vec<Circle>> = use_signal(Vec::new);
    // undo stack: each entry is a full snapshot of circles vec
    let mut undo_stack: Signal<Vec<Vec<Circle>>> = use_signal(Vec::new);
    let mut redo_stack: Signal<Vec<Vec<Circle>>> = use_signal(Vec::new);
    // index of circle being resized, if dialog is open
    let mut dialog_idx: Signal<Option<usize>> = use_signal(|| None);
    let mut dialog_r: Signal<f64> = use_signal(|| 20.0);
    // Captured at mount; rect is read at click time because Blitz fires
    // `mounted` before layout, and `element_coordinates()` is unimplemented.
    let mut canvas_handle: Signal<Option<Rc<MountedData>>> = use_signal(|| None);

    rsx! {
        div { class: "cd-root",
            style { {CSS} }
            div { class: "cd-toolbar",
                button {
                    disabled: undo_stack().is_empty(),
                    onclick: move |_| {
                        if let Some(prev) = undo_stack.write().pop() {
                            redo_stack.write().push(circles());
                            circles.set(prev);
                        }
                    },
                    "Undo"
                }
                button {
                    disabled: redo_stack().is_empty(),
                    onclick: move |_| {
                        if let Some(next) = redo_stack.write().pop() {
                            undo_stack.write().push(circles());
                            circles.set(next);
                        }
                    },
                    "Redo"
                }
            }
            // Canvas area
            div {
                class: "cd-canvas",
                onmounted: move |evt: MountedEvent| {
                    canvas_handle.set(Some(evt.data()));
                },
                onclick: move |evt| async move {
                    let raw = evt.page_coordinates();
                    let (ox, oy) = match canvas_handle() {
                        Some(h) => h.get_client_rect().await
                            .map(|r| (r.origin.x, r.origin.y))
                            .unwrap_or((0.0, 0.0)),
                        None => (0.0, 0.0),
                    };
                    let x = raw.x - ox;
                    let y = raw.y - oy;
                    undo_stack.write().push(circles());
                    redo_stack.write().clear();
                    circles.write().push(Circle { x, y, r: 20.0 });
                },
                ondoubleclick: move |evt| async move {
                    evt.stop_propagation();
                    let raw = evt.page_coordinates();
                    let (ox, oy) = match canvas_handle() {
                        Some(h) => h.get_client_rect().await
                            .map(|r| (r.origin.x, r.origin.y))
                            .unwrap_or((0.0, 0.0)),
                        None => (0.0, 0.0),
                    };
                    let cx = raw.x - ox;
                    let cy = raw.y - oy;
                    let nearest = circles().iter().enumerate().min_by(|(_, a), (_, b)| {
                        let da = (a.x - cx).powi(2) + (a.y - cy).powi(2);
                        let db = (b.x - cx).powi(2) + (b.y - cy).powi(2);
                        da.total_cmp(&db)
                    }).map(|(i, c)| (i, c.r));
                    if let Some((i, r)) = nearest {
                        dialog_idx.set(Some(i));
                        dialog_r.set(r);
                    }
                },
                for (i, circle) in circles().iter().enumerate() {
                    div {
                        key: "{i}",
                        class: "circle",
                        style: "left: {circle.x - circle.r}px; top: {circle.y - circle.r}px; width: {circle.r * 2.0}px; height: {circle.r * 2.0}px;",
                    }
                }
            }
            // Diameter adjustment dialog
            if let Some(idx) = dialog_idx() {
                div { class: "cd-dialog",
                    p { "Adjust diameter of circle #{idx}" }
                    input {
                        r#type: "range",
                        min: "5",
                        max: "100",
                        value: "{dialog_r() * 2.0}",
                        oninput: move |evt| {
                            if let Ok(d) = evt.value().parse::<f64>() {
                                let d = d.clamp(5.0, 100.0);
                                let new_r = d / 2.0;
                                dialog_r.set(new_r);
                                undo_stack.write().push(circles());
                                redo_stack.write().clear();
                                if let Some(c) = circles.write().get_mut(idx) {
                                    c.r = new_r;
                                }
                            }
                        }
                    }
                    p { "Diameter: {(dialog_r() * 2.0) as u32}px" }
                    button {
                        onclick: move |_| dialog_idx.set(None),
                        "Close"
                    }
                }
            }
        }
    }
}

const CSS: &str = r#"
.cd-root {
    display: flex;
    flex-direction: column;
    align-items: center;
    width: 100%;
    height: 100%;
    background-color: #f5f5f5;
    font-family: sans-serif;
    padding: 16px;
    box-sizing: border-box;
}

.cd-toolbar {
    display: flex;
    flex-direction: row;
    gap: 12px;
    margin-bottom: 12px;
}

.cd-toolbar button {
    font-size: 14px;
    font-weight: 500;
    color: #ffffff;
    background-color: #4a6cf7;
    border: none;
    border-radius: 6px;
    padding: 8px 20px;
    cursor: pointer;
}

.cd-toolbar button:hover {
    background-color: #3a5ce5;
}

.cd-toolbar button:disabled {
    background-color: #a0a0a0;
    cursor: default;
}

.cd-canvas {
    position: relative;
    width: 700px;
    height: 400px;
    background-color: #ffffff;
    border: 2px solid #c0c0c0;
    border-radius: 4px;
    overflow: hidden;
    cursor: crosshair;
}

.circle {
    position: absolute;
    border-radius: 50%;
    background-color: rgba(74, 108, 247, 0.15);
    border: 2px solid #4a6cf7;
    box-sizing: border-box;
    pointer-events: none;
}

.circle:hover {
    background-color: rgba(74, 108, 247, 0.35);
}

.cd-dialog {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    margin-top: 16px;
    background: #ffffff;
    border: 1px solid #d0d0d0;
    border-radius: 8px;
    padding: 20px 32px;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.10);
    min-width: 320px;
}

.cd-dialog p {
    margin: 0;
    font-size: 14px;
    color: #333333;
}

.cd-dialog input[type="range"] {
    width: 240px;
}

.cd-dialog button {
    font-size: 14px;
    font-weight: 500;
    color: #ffffff;
    background-color: #4a6cf7;
    border: none;
    border-radius: 6px;
    padding: 8px 20px;
    cursor: pointer;
}

.cd-dialog button:hover {
    background-color: #3a5ce5;
}
"#;
