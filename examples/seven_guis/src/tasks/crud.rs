use dioxus_native::prelude::*;

#[derive(Clone, PartialEq)]
struct Person {
    first: String,
    last: String,
}

#[component]
pub fn Crud() -> Element {
    let mut people = use_signal(|| {
        vec![
            Person {
                first: "Hans".into(),
                last: "Emil".into(),
            },
            Person {
                first: "Max".into(),
                last: "Mustermann".into(),
            },
            Person {
                first: "Roman".into(),
                last: "Tisch".into(),
            },
        ]
    });
    let mut selected: Signal<Option<usize>> = use_signal(|| None);
    let mut filter = use_signal(String::new);
    let mut first_field = use_signal(String::new);
    let mut last_field = use_signal(String::new);

    let has_selection = selected().is_some();

    rsx! {
        div { class: "crud-root",
            style { {CSS} }
            // Filter row
            div { class: "row",
                label { "Filter prefix: " }
                input {
                    value: "{filter}",
                    oninput: move |e| {
                        selected.set(None);
                        filter.set(e.value());
                    }
                }
            }
            // List + fields side by side
            div { class: "crud-body",
                // List
                div { class: "list",
                    {
                        let filter_lower = filter().to_lowercase();
                        let people_snap = people();
                        rsx! {
                            for (i, person) in people_snap.iter().enumerate() {
                                if format!("{}, {}", person.last, person.first)
                                    .to_lowercase()
                                    .starts_with(&filter_lower)
                                {
                                    div {
                                        class: if selected() == Some(i) { "list-item selected" } else { "list-item" },
                                        onclick: move |_| {
                                            if let Some(p) = people.read().get(i).cloned() {
                                                selected.set(Some(i));
                                                first_field.set(p.first);
                                                last_field.set(p.last);
                                            }
                                        },
                                        "{person.last}, {person.first}"
                                    }
                                }
                            }
                        }
                    }
                }
                // Fields
                div { class: "fields",
                    label { "Name: " }
                    input {
                        value: "{first_field}",
                        oninput: move |e| first_field.set(e.value())
                    }
                    label { "Surname: " }
                    input {
                        value: "{last_field}",
                        oninput: move |e| last_field.set(e.value())
                    }
                }
            }
            // Buttons
            div { class: "crud-buttons",
                button {
                    onclick: move |_| {
                        people.write().push(Person { first: first_field(), last: last_field() });
                    },
                    "Create"
                }
                button {
                    class: if has_selection { "" } else { "btn-off" },
                    disabled: !has_selection,
                    onclick: move |_| {
                        if let Some(idx) = selected() {
                            let mut p = people.write();
                            if let Some(entry) = p.get_mut(idx) {
                                *entry = Person { first: first_field(), last: last_field() };
                            }
                        }
                    },
                    "Update"
                }
                button {
                    class: if has_selection { "" } else { "btn-off" },
                    disabled: !has_selection,
                    onclick: move |_| {
                        if let Some(idx) = selected() {
                            let mut p = people.write();
                            if idx < p.len() {
                                p.remove(idx);
                            }
                            selected.set(None);
                        }
                    },
                    "Delete"
                }
            }
        }
    }
}

const CSS: &str = r#"
.crud-root {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 24px;
    font-family: sans-serif;
    font-size: 14px;
    background-color: #f5f5f5;
    width: 100%;
    height: 100%;
    box-sizing: border-box;
}

.row {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 8px;
}

.crud-body {
    display: flex;
    flex-direction: row;
    gap: 16px;
    flex: 1;
    min-height: 0;
}

.list {
    display: flex;
    flex-direction: column;
    flex: 1;
    border: 1px solid #c0c0c0;
    border-radius: 4px;
    background: #ffffff;
    overflow-y: auto;
    min-height: 150px;
}

.list-item {
    padding: 6px 10px;
    cursor: pointer;
    border-bottom: 1px solid #eeeeee;
    color: #1a1a1a;
}

.list-item:hover {
    background-color: #e8edf8;
}

.list-item.selected {
    background-color: #4a6cf7;
    color: #ffffff;
}

.fields {
    display: flex;
    flex-direction: column;
    gap: 8px;
    flex: 1;
    justify-content: flex-start;
}

.fields label {
    font-weight: 500;
    color: #333333;
}

.fields input {
    padding: 6px 8px;
    border: 1px solid #c0c0c0;
    border-radius: 4px;
    font-size: 14px;
}

.crud-buttons {
    display: flex;
    flex-direction: row;
    gap: 8px;
}

.crud-buttons button {
    padding: 8px 18px;
    font-size: 14px;
    font-weight: 500;
    color: #ffffff;
    background-color: #4a6cf7;
    border: none;
    border-radius: 5px;
    cursor: pointer;
}

.crud-buttons button:hover:not(.btn-off) {
    background-color: #3a5ce5;
}

.crud-buttons button.btn-off {
    background-color: #b0b8d8;
    cursor: default;
}

input {
    padding: 6px 8px;
    border: 1px solid #c0c0c0;
    border-radius: 4px;
    font-size: 14px;
}
"#;
