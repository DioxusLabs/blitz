use dioxus_native::prelude::*;

use crate::tasks::{cells, circle_drawer, counter, crud, flight_booker, temp_converter, timer};

#[derive(Clone, Copy, PartialEq)]
enum Task {
    Counter,
    TempConverter,
    FlightBooker,
    Timer,
    Crud,
    CircleDrawer,
    Cells,
}

struct TaskMeta {
    task: Task,
    name: &'static str,
    description: &'static str,
    tag: &'static str,
}

const TASKS: &[TaskMeta] = &[
    TaskMeta {
        task: Task::Counter,
        name: "Counter",
        description: "Increment a count. Tests basic state mutation.",
        tag: "State",
    },
    TaskMeta {
        task: Task::TempConverter,
        name: "Temp Converter",
        description: "Celsius \u{21c4} Fahrenheit. Tests bidirectional data flow.",
        tag: "Data Flow",
    },
    TaskMeta {
        task: Task::FlightBooker,
        name: "Flight Booker",
        description: "One-way or return flight form. Tests constraint logic.",
        tag: "Constraints",
    },
    TaskMeta {
        task: Task::Timer,
        name: "Timer",
        description: "Elapsed time bar with adjustable duration. Tests concurrency.",
        tag: "Concurrency",
    },
    TaskMeta {
        task: Task::Crud,
        name: "CRUD",
        description: "Create, read, update, delete names. Tests list management.",
        tag: "List Ops",
    },
    TaskMeta {
        task: Task::CircleDrawer,
        name: "Circle Drawer",
        description: "Draw and resize circles with undo/redo. Tests history.",
        tag: "Undo / Redo",
    },
    TaskMeta {
        task: Task::Cells,
        name: "Cells",
        description: "Mini spreadsheet with formula evaluation. Tests reactivity.",
        tag: "Reactivity",
    },
];

pub fn app() -> Element {
    let mut active: Signal<Option<Task>> = use_signal(|| None);

    match active() {
        None => rsx! {
            style { {HOME_CSS} }
            Home { on_select: move |t| active.set(Some(t)) }
        },
        Some(Task::Counter) => rsx! {
            TaskShell { title: "Counter", on_back: move |_| active.set(None),
                counter::Counter {}
            }
        },
        Some(Task::TempConverter) => rsx! {
            TaskShell { title: "Temperature Converter", on_back: move |_| active.set(None),
                temp_converter::TempConverter {}
            }
        },
        Some(Task::FlightBooker) => rsx! {
            TaskShell { title: "Flight Booker", on_back: move |_| active.set(None),
                flight_booker::FlightBooker {}
            }
        },
        Some(Task::Timer) => rsx! {
            TaskShell { title: "Timer", on_back: move |_| active.set(None),
                timer::Timer {}
            }
        },
        Some(Task::Crud) => rsx! {
            TaskShell { title: "CRUD", on_back: move |_| active.set(None),
                crud::Crud {}
            }
        },
        Some(Task::CircleDrawer) => rsx! {
            TaskShell { title: "Circle Drawer", on_back: move |_| active.set(None),
                circle_drawer::CircleDrawer {}
            }
        },
        Some(Task::Cells) => rsx! {
            TaskShell { title: "Cells", on_back: move |_| active.set(None),
                cells::Cells {}
            }
        },
    }
}

#[component]
fn Home(on_select: EventHandler<Task>) -> Element {
    rsx! {
        div { id: "home",
            header { id: "home-header",
                h1 { id: "home-title", "7GUIs" }
                p { id: "home-subtitle", "Seven benchmark tasks for GUI frameworks" }
            }
            div { id: "task-grid",
                for (i, meta) in TASKS.iter().enumerate() {
                    {
                        let task = meta.task;
                        rsx! {
                            button {
                                class: "task-card",
                                onclick: move |_| on_select.call(task),
                                div { class: "card-number", "{i + 1}" }
                                div { class: "card-body",
                                    h2 { class: "card-name", "{meta.name}" }
                                    p { class: "card-desc", "{meta.description}" }
                                }
                                span { class: "card-tag", "{meta.tag}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TaskShell(title: &'static str, on_back: EventHandler<()>, children: Element) -> Element {
    rsx! {
        style { {SHELL_CSS} }
        div { id: "task-shell",
            header { id: "task-header",
                button {
                    id: "back-btn",
                    onclick: move |_| on_back.call(()),
                    "\u{2190} Back"
                }
                h1 { id: "task-title", "{title}" }
                div { id: "task-header-spacer" }
            }
            main { id: "task-body",
                {children}
            }
        }
    }
}

const HOME_CSS: &str = r#"
* { box-sizing: border-box; margin: 0; padding: 0; }

html, body, #main {
    height: 100%;
    font-family: sans-serif;
    font-size: 14px;
    background: #f5f5f5;
}

#home {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow-y: auto;
    padding: 48px 32px 64px;
    background: #f5f5f5;
}

#home-header {
    text-align: center;
    margin-bottom: 40px;
}

#home-title {
    font-size: 36px;
    font-weight: 700;
    color: #111;
    margin-bottom: 8px;
}

#home-subtitle {
    font-size: 14px;
    color: #666;
}

#task-grid {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: 640px;
    margin: 0 auto;
    width: 100%;
}

.task-card {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 16px;
    background: #fff;
    border: 1px solid #ddd;
    border-radius: 6px;
    padding: 16px 20px;
    text-align: left;
    cursor: pointer;
    color: inherit;
    font-family: inherit;
    width: 100%;
}

.task-card:hover {
    background: #f0f0f0;
}

.card-number {
    font-size: 16px;
    font-weight: 600;
    color: #999;
    min-width: 20px;
    text-align: center;
}

.card-body {
    flex: 1;
}

.card-name {
    font-size: 15px;
    font-weight: 600;
    color: #111;
    margin-bottom: 2px;
}

.card-desc {
    font-size: 13px;
    color: #666;
}

.card-tag {
    font-size: 11px;
    color: #888;
    border: 1px solid #ccc;
    border-radius: 3px;
    padding: 2px 6px;
    white-space: nowrap;
}
"#;

const SHELL_CSS: &str = r#"
* { box-sizing: border-box; margin: 0; padding: 0; }

html, body, #main {
    height: 100%;
    font-family: sans-serif;
    font-size: 14px;
}

#task-shell {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: #f5f5f5;
}

#task-header {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 16px;
    background: #fff;
    padding: 10px 16px;
    border-bottom: 1px solid #ddd;
}

#back-btn {
    font-size: 13px;
    color: #333;
    background: #f0f0f0;
    border: 1px solid #ccc;
    border-radius: 4px;
    padding: 4px 12px;
    cursor: pointer;
    white-space: nowrap;
}

#back-btn:hover {
    background: #e4e4e4;
}

#task-title {
    font-size: 15px;
    font-weight: 600;
    color: #111;
}

#task-header-spacer {
    flex: 1;
}

#task-body {
    flex: 1;
    overflow: auto;
}
"#;
