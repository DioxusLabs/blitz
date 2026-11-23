//! A kanban board that is rearranged by dragging
//!
//! - Drag a card by its title into another column. "Done" refuses blocked cards.
//! - Drag a card out to a text editor to drop it there as text.
//! - Select some of a card's description and drag it onto the note field of any card.

use dioxus_native::prelude::*;

/// The type of the dragged data that identifies a card to the columns
const CARD_TYPE: &str = "application/x-card";

const COLUMNS: [&str; 3] = ["To do", "In progress", "Done"];
const DONE: usize = 2;

fn main() {
    dioxus_native::launch(app);
}

#[derive(Clone, PartialEq)]
struct Card {
    id: u32,
    column: usize,
    title: &'static str,
    description: &'static str,
    blocked: bool,
    note: String,
}

/// The column, title and description of each card, and whether it is blocked
#[rustfmt::skip]
const CARDS: [(usize, &str, &str, bool); 6] = [
    (0, "Design the icon", "Three sizes, light and dark.", false),
    (0, "Write the release notes", "Waiting for the final list of changes.", true),
    (0, "Update the screenshots", "The settings page has changed.", false),
    (1, "Fix the login form", "It loses focus after a failed attempt.", false),
    (1, "Upgrade the dependencies", "Blocked on a release of the parser.", true),
    (2, "Set up CI", "Builds and tests on every pull request.", false),
];

#[derive(Clone, Copy)]
struct Board {
    cards: Signal<Vec<Card>>,
    /// The id of the card being dragged. Handlers can't read the dragged data until the
    /// drop, so this is what tells a column which card is over it.
    dragged: Signal<Option<u32>>,
    /// The card that was dropped, and the column it was dropped on
    dropped: Signal<Option<(u32, usize)>>,
    last_drag: Signal<String>,
}

fn app() -> Element {
    let board = use_context_provider(|| Board {
        cards: Signal::new(
            (0..)
                .zip(CARDS)
                .map(|(id, (column, title, description, blocked))| Card {
                    id,
                    column,
                    title,
                    description,
                    blocked,
                    note: String::new(),
                })
                .collect(),
        ),
        dragged: Signal::new(None),
        dropped: Signal::new(None),
        last_drag: Signal::new(String::new()),
    });

    rsx! {
        style { {CSS} }
        main {
            h1 { "Kanban" }
            p { class: "hint",
                "Drag a card by its title to move it, or select some of a description and drag it onto a note."
            }
            div { class: "board",
                for column in 0..COLUMNS.len() {
                    Column { column }
                }
            }
            p { class: "hint", "{board.last_drag}" }
        }
    }
}

#[component]
fn Column(column: usize) -> Element {
    let Board {
        cards,
        dragged,
        mut dropped,
        ..
    } = use_context();
    // `dragenter` and `dragleave` bubble up from the cards in the column, so count them
    // rather than tracking a single flag
    let mut drag_depth = use_signal(|| 0u32);

    // Whether the column takes the card being dragged, if there is one
    let accepts = use_memo(move || {
        let id = dragged()?;
        let blocked = cards
            .read()
            .iter()
            .any(|card| card.id == id && card.blocked);
        Some(!(column == DONE && blocked))
    });
    let class = match accepts() {
        Some(true) if drag_depth() > 0 => "column accepts",
        Some(false) if drag_depth() > 0 => "column refuses",
        _ => "column",
    };

    rsx! {
        div {
            class,
            ondragenter: move |_| drag_depth += 1,
            ondragleave: move |_| drag_depth.set(drag_depth().saturating_sub(1)),
            ondragover: move |evt| {
                if accepts() == Some(true) {
                    // Cancelling `dragover` is what accepts a drop
                    evt.prevent_default();
                    evt.data_transfer().set_drop_effect("move");
                }
            },
            ondrop: move |evt| {
                drag_depth.set(0);
                let id = evt.data_transfer().get_data(CARD_TYPE);
                if let Some(id) = id.and_then(|id| id.parse().ok()) {
                    evt.prevent_default();
                    // The card moves once its `dragend` has fired: moving it now would remove
                    // the element that `dragend` is sent to
                    dropped.set(Some((id, column)));
                }
            },
            h2 { "{COLUMNS[column]}" }
            for card in cards.read().iter().filter(|card| card.column == column) {
                CardView { key: "{card.id}", card: card.clone() }
            }
            if class == "column refuses" {
                p { class: "refusal", "Blocked cards can't be done" }
            }
        }
    }
}

#[component]
fn CardView(card: Card) -> Element {
    let Board {
        mut cards,
        mut dragged,
        mut dropped,
        mut last_drag,
    } = use_context();
    let Card {
        id,
        title,
        description,
        ..
    } = card;
    let mut set_note = move |note: String| {
        if let Some(card) = cards.write().iter_mut().find(|card| card.id == id) {
            card.note = note;
        }
    };

    rsx! {
        div { class: if dragged() == Some(id) { "card dragged" } else { "card" },
            div {
                class: "title",
                draggable: "true",
                ondragstart: move |evt| {
                    let data = evt.data_transfer();
                    let _ = data.set_data(CARD_TYPE, &id.to_string());
                    // What another application gets
                    let _ = data.set_data("text/plain", &format!("{title}\n{description}"));
                    dragged.set(Some(id));
                },
                ondragend: move |evt| {
                    dragged.set(None);
                    last_drag.set(format!("Dragging \"{title}\" ended with: {}", evt.data_transfer().drop_effect()));

                    if let Some((id, column)) = dropped.take() {
                        let mut cards = cards.write();
                        if let Some(index) = cards.iter().position(|card| card.id == id) {
                            let mut card = cards.remove(index);
                            card.column = column;
                            cards.push(card);
                        }
                    }
                },
                span { "{title}" }
                if card.blocked {
                    span { class: "tag", "Blocked" }
                }
            }
            p { class: "description", "{description}" }
            label {
                "Note"
                input {
                    value: "{card.note}",
                    oninput: move |evt| set_note(evt.value()),
                    // Text is dropped here, but a card is left to the column
                    ondragover: move |evt| {
                        if dragged().is_none() {
                            evt.prevent_default();
                        }
                    },
                    ondrop: move |evt| {
                        if dragged().is_none()
                            && let Some(text) = evt.data_transfer().get_data("text/plain")
                        {
                            evt.prevent_default();
                            set_note(text);
                        }
                    },
                }
            }
        }
    }
}

const CSS: &str = r#"
    html { background: #f3f4f6; }
    body { margin: 0; font-family: system-ui, sans-serif; color: #1f2937; }
    main { max-width: 900px; margin: 0 auto; padding: 24px; }
    h1 { margin: 0; font-size: 22px; }
    .hint { min-height: 18px; margin: 8px 0 16px; color: #6b7280; font-size: 13px; }

    .board { display: flex; align-items: flex-start; gap: 16px; }
    .column {
        flex: 1;
        min-width: 0;
        min-height: 200px;
        padding: 12px;
        border: 2px solid transparent;
        border-radius: 10px;
        background: #e5e7eb;
    }
    .column.accepts { border-color: #2563eb; background: #dbeafe; }
    .column.refuses { border: 2px dashed #dc2626; background: #fee2e2; }
    .column h2 { margin: 0 0 12px; font-size: 14px; text-transform: uppercase; color: #4b5563; }
    .refusal { margin: 0; color: #b91c1c; font-size: 13px; }

    .card {
        margin-bottom: 10px;
        border-radius: 8px;
        background: white;
        box-shadow: 0 1px 2px rgba(0, 0, 0, 0.2);
    }
    .card.dragged { opacity: 0.4; }
    .card .title {
        display: flex;
        justify-content: space-between;
        align-items: center;
        gap: 8px;
        padding: 10px 10px 4px;
        font-weight: 600;
        cursor: grab;
        user-select: none;
    }
    .card .title span:first-child { flex: 1; min-width: 0; }
    .card .tag {
        flex: none;
        padding: 2px 8px;
        border-radius: 999px;
        background: #fee2e2;
        color: #b91c1c;
        font-size: 11px;
    }
    .card .description { margin: 0; padding: 0 10px; color: #4b5563; font-size: 13px; }
    .card label {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 8px 10px 10px;
        color: #6b7280;
        font-size: 12px;
    }
    .card input {
        flex: 1;
        min-width: 0;
        padding: 4px 6px;
        border: 1px solid #d1d5db;
        border-radius: 4px;
        font-size: 12px;
    }
"#;
