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

#[derive(Clone, Copy, PartialEq)]
struct Board {
    cards: Signal<Vec<Card>>,
    /// The id of the card being dragged. Handlers can't read the dragged data until the
    /// drop, so this is what tells a column which card is over it.
    dragged: Signal<Option<u32>>,
    /// Where the dragged card would be dropped: a column, and the number of its other
    /// cards that would be above it
    target: Signal<Option<(usize, usize)>>,
    /// The card that was dropped, and where
    dropped: Signal<Option<(u32, usize, usize)>>,
    last_drag: Signal<String>,
}

impl Board {
    /// The cards of a column other than the one being dragged
    fn others(&self, column: usize) -> Vec<Card> {
        let dragged = (self.dragged)();
        let cards = self.cards.read();
        let others = cards
            .iter()
            .filter(|card| card.column == column && Some(card.id) != dragged);
        others.cloned().collect()
    }

    /// Whether a column takes the card being dragged, if there is one
    fn accepts(&self, column: usize) -> Option<bool> {
        let id = (self.dragged)()?;
        let blocked = self
            .cards
            .read()
            .iter()
            .any(|card| card.id == id && card.blocked);
        Some(!(column == DONE && blocked))
    }

    /// Move the placeholder of the dragged card to where a drop would put it
    fn set_target(&mut self, target: Option<(usize, usize)>) {
        let target = target.filter(|(column, _)| self.accepts(*column) == Some(true));
        if (self.target)() != target {
            self.target.set(target);
        }
    }

    fn move_card(&mut self, id: u32, column: usize, index: usize) {
        let mut cards = self.cards.write();
        let Some(from) = cards.iter().position(|card| card.id == id) else {
            return;
        };
        let mut card = cards.remove(from);
        card.column = column;
        let in_column = cards
            .iter()
            .enumerate()
            .filter(|(_, card)| card.column == column);
        let to = in_column
            .map(|(to, _)| to)
            .nth(index)
            .unwrap_or(cards.len());
        cards.insert(to, card);
    }
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
        target: Signal::new(None),
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
    let mut board: Board = use_context();
    // `dragenter` and `dragleave` bubble up from the cards in the column, so count them
    // rather than tracking a single flag
    let mut drag_depth = use_signal(|| 0u32);

    let accepts = board.accepts(column);
    let class = match accepts {
        Some(true) if drag_depth() > 0 => "column accepts",
        Some(false) if drag_depth() > 0 => "column refuses",
        _ => "column",
    };

    // While a card is dragged it is hidden, and a placeholder for it is shown where it
    // would be dropped. The card itself has to stay where it is until its `dragend`.
    let dragged = (board.dragged)();
    let target = (board.target)();
    let cards = board.cards.read().clone();
    let dragged_card = cards.iter().find(|card| Some(card.id) == dragged);
    let mut slots = Vec::new();
    let mut index = 0;
    for card in cards.iter().filter(|card| card.column == column) {
        if Some(card.id) == dragged {
            slots.push((card.clone(), Slot::Dragged));
            continue;
        }
        if let Some(card) = dragged_card.filter(|_| target == Some((column, index))) {
            slots.push((card.clone(), Slot::Placeholder));
        }
        slots.push((card.clone(), Slot::Card(index)));
        index += 1;
    }
    if let Some(card) = dragged_card.filter(|_| target == Some((column, index))) {
        slots.push((card.clone(), Slot::Placeholder));
    }

    rsx! {
        div {
            class,
            ondragenter: move |_| drag_depth += 1,
            ondragleave: move |_| {
                drag_depth.set(drag_depth().saturating_sub(1));
                if drag_depth() == 0 && target.is_some_and(|target| target.0 == column) {
                    board.set_target(None);
                }
            },
            ondragover: move |evt| {
                // The cards have already put the placeholder next to the one the drag is
                // over. Anywhere else in the column is the end of it.
                if (board.target)().is_none_or(|target| target.0 != column) {
                    board.set_target(Some((column, board.others(column).len())));
                }
                if accepts == Some(true) {
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
                    let index = match (board.target)() {
                        Some((target, index)) if target == column => index,
                        _ => board.others(column).len(),
                    };
                    board.dropped.set(Some((id, column, index)));
                }
            },
            h2 { "{COLUMNS[column]}" }
            for (card , slot) in slots {
                CardView { key: "{slot.key(card.id)}", card, slot }
            }
            if class == "column refuses" {
                p { class: "refusal", "Blocked cards can't be done" }
            }
            // The rest of the column is the end of it
            div {
                class: "end",
                ondragover: move |_| board.set_target(Some((column, board.others(column).len()))),
            }
        }
    }
}

/// How a card is shown in its column
#[derive(Clone, Copy, PartialEq)]
enum Slot {
    /// A card that is not being dragged, and the number of such cards above it
    Card(usize),
    /// The card that is being dragged, where it was when the drag started
    Dragged,
    /// A copy of the card that is being dragged, where it would be dropped
    Placeholder,
}

impl Slot {
    fn key(self, id: u32) -> String {
        match self {
            Slot::Placeholder => "placeholder".to_string(),
            _ => id.to_string(),
        }
    }
}

#[component]
fn CardView(card: Card, slot: Slot) -> Element {
    let mut board: Board = use_context();
    let Card {
        id,
        column,
        title,
        description,
        ..
    } = card;
    let mut set_note = move |note: String| {
        if let Some(card) = board.cards.write().iter_mut().find(|card| card.id == id) {
            card.note = note;
        }
    };
    let dragging = (board.dragged)().is_some();
    let class = match slot {
        Slot::Card(_) => "card",
        Slot::Dragged if (board.target)().is_some() => "card hidden",
        Slot::Dragged | Slot::Placeholder => "card placeholder",
    };

    rsx! {
        div { class,
            div {
                class: "title",
                draggable: if slot == Slot::Placeholder { "false" } else { "true" },
                ondragstart: move |evt| {
                    let data = evt.data_transfer();
                    let _ = data.set_data(CARD_TYPE, &id.to_string());
                    // What another application gets
                    let _ = data.set_data("text/plain", &format!("{title}\n{description}"));
                    board.dragged.set(Some(id));
                    // The placeholder starts out where the card is
                    if let Slot::Card(index) = slot {
                        board.target.set(Some((column, index)));
                    }
                },
                ondragend: move |evt| {
                    board.dragged.set(None);
                    board.target.set(None);
                    let effect = evt.data_transfer().drop_effect();
                    board.last_drag.set(format!("Dragging \"{title}\" ended with: {effect}"));

                    if let Some((id, column, index)) = board.dropped.take() {
                        board.move_card(id, column, index);
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
                        if !dragging {
                            evt.prevent_default();
                        }
                    },
                    ondrop: move |evt| {
                        if !dragging
                            && let Some(text) = evt.data_transfer().get_data("text/plain")
                        {
                            evt.prevent_default();
                            set_note(text);
                        }
                    },
                }
            }
            // A card dragged over the top half of this one would be dropped above it, and
            // over the bottom half below it
            if let Slot::Card(index) = slot
                && dragging
            {
                div {
                    class: "half top",
                    ondragover: move |_| board.set_target(Some((column, index))),
                }
                div {
                    class: "half bottom",
                    ondragover: move |_| board.set_target(Some((column, index + 1))),
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
        display: flex;
        flex-direction: column;
        min-width: 0;
        min-height: 200px;
        padding: 12px 12px 0;
        border: 2px solid transparent;
        border-radius: 10px;
        background: #e5e7eb;
    }
    .column.accepts { border-color: #2563eb; background: #dbeafe; }
    .column.refuses { border: 2px dashed #dc2626; background: #fee2e2; }
    .column h2 { margin: 0 0 12px; font-size: 14px; text-transform: uppercase; color: #4b5563; }
    .refusal { margin: 0; color: #b91c1c; font-size: 13px; }
    .column .end { flex: 1; min-height: 12px; }

    .card {
        position: relative;
        margin-bottom: 10px;
        border-radius: 8px;
        background: white;
        box-shadow: 0 1px 2px rgba(0, 0, 0, 0.2);
    }
    .card.placeholder { opacity: 0.4; }
    .card.hidden { display: none; }
    .card .half { position: absolute; left: 0; right: 0; height: 50%; }
    .card .half.top { top: 0; }
    .card .half.bottom { bottom: 0; }
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
