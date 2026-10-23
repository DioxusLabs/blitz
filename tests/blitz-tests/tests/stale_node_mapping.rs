//! Regression tests for node removal in `dioxus-native-dom`.
//! See https://github.com/DioxusLabs/dioxus/pull/5182
//!
//! Removing a node used to leave `ElementId -> NodeId` mappings for its
//! subtree behind. When blitz-dom reused the freed slot for a new node and
//! dioxus-core reused the stale `ElementId`, the writer dropped the new node
//! and a later mutation in the same render panicked with "invalid key".
//!
//! Node ids are now generational and dioxus-core always re-maps an
//! `ElementId` (`set_id`) before pushing it, so these tests only check that
//! heavy template, keyed-list and `ElementId` churn renders without panicking.

use blitz_test_harness::HarnessOptions;
use dioxus::prelude::*;
use dioxus_core::ScopeId;
use std::cell::Cell;
use std::rc::Rc;

#[derive(Props, Clone)]
struct AppProps {
    generation: Rc<Cell<usize>>,
    view: fn(usize) -> Element,
}

impl PartialEq for AppProps {
    fn eq(&self, other: &Self) -> bool {
        self.generation == other.generation && std::ptr::fn_addr_eq(self.view, other.view)
    }
}

fn app(props: AppProps) -> Element {
    (props.view)(props.generation.get())
}

struct Harness {
    inner: blitz_test_harness::Harness<DioxusDocument>,
    generation: Rc<Cell<usize>>,
}

impl Harness {
    fn new(view: fn(usize) -> Element) -> Self {
        let generation = Rc::new(Cell::new(0));
        let vdom = VirtualDom::new_with_props(
            app,
            AppProps {
                generation: Rc::clone(&generation),
                view,
            },
        );
        let inner = blitz_test_harness::Harness::from_vdom(vdom, HarnessOptions::default());
        Self { inner, generation }
    }

    fn step(&mut self) {
        self.generation.set(self.generation.get() + 1);
        self.inner.doc.vdom.mark_dirty(ScopeId::APP);
        self.inner.pump();
    }
}

fn bare_text(generation: usize) -> Element {
    rsx! { "bare text {generation}" }
}

fn section_with_list(generation: usize, n: usize) -> Element {
    rsx! {
        section {
            header { id: "gen-{generation}", "header" }
            for i in 0..n {
                div { key: "{i}", "item {i}" }
            }
        }
    }
}

fn nested_divs(generation: usize) -> Element {
    rsx! {
        article {
            div { class: "gen-{generation}",
                div { id: "gen-{generation}", "deep {generation}" }
            }
        }
        aside { "aside" }
    }
}

/// Minimal deterministic sequence of template swaps that panics with
/// "invalid key" inside blitz-dom's `DocumentMutator` (in `node_at_path`
/// via `assign_node_id`) on the 3rd re-render.
fn minimal_panic_app(generation: usize) -> Element {
    match generation {
        0 => nested_divs(generation),
        1 => bare_text(generation),
        2 => section_with_list(generation, 0),
        _ => nested_divs(generation),
    }
}

#[test]
fn template_swaps_do_not_panic() {
    let mut harness = Harness::new(minimal_panic_app);
    for _ in 0..3 {
        harness.step();
    }
}

/// Toggles between templates where one variant has a nested element with a
/// dynamic attribute (which is assigned an ElementId via `AssignId`).
/// Detects the stale mapping (the precursor to the panic) directly.
fn toggle_app(generation: usize) -> Element {
    let class = format!("gen-{generation}");
    match generation % 3 {
        0 => rsx! {
            div {
                div { class: "{class}",
                    span { "nested {generation}" }
                }
                "text {generation}"
            }
            div { "sibling" }
        },
        1 => rsx! {
            p { "different template {generation}" }
        },
        _ => rsx! {
            section {
                header { id: "{class}", "header" }
                for i in 0..(generation % 5) {
                    div { key: "{i}", "item {i}" }
                }
            }
        },
    }
}

#[test]
fn template_swaps_do_not_panic_over_many_steps() {
    let mut harness = Harness::new(toggle_app);
    for _ in 0..20 {
        harness.step();
    }
}

/// Pseudo-random mix of templates designed to maximise ElementId and NodeId
/// (slab slot) reuse. Panics with "invalid key" within a handful of steps.
fn fuzz_app(generation: usize) -> Element {
    // Simple LCG for deterministic pseudo-random variety
    let r = generation
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let variant = (r >> 33) % 6;
    let n = (r >> 40) % 7;
    let class = format!("gen-{generation}");
    match variant {
        0 => rsx! {
            div {
                div { class: "{class}", span { "nested {generation}" } }
                "text {generation}"
            }
        },
        1 => rsx! {
            p { "different template {generation}" }
        },
        2 => section_with_list(generation, n),
        3 => rsx! {
            ul {
                for i in 0..n {
                    li { key: "{i}", class: "item-{i}-{generation}",
                        span { "item {i} gen {generation}" }
                    }
                }
            }
        },
        4 => nested_divs(generation),
        _ => bare_text(generation),
    }
}

#[test]
fn fuzz_template_swaps_do_not_panic() {
    let mut harness = Harness::new(fuzz_app);
    for _ in 0..1000 {
        harness.step();
    }
}

/// A keyed list where items are removed and re-added, forcing ElementId reuse
/// in dioxus-core and NodeId (slab slot) reuse in blitz-dom.
fn list_app(generation: usize) -> Element {
    // Vary the list length up and down so nodes are removed and recreated
    let len = [5usize, 0, 3, 1, 6, 2, 0, 4][generation % 8];
    rsx! {
        ul {
            for i in 0..len {
                li { key: "{i}", class: "item-{i}-{generation}",
                    span { "item {i} gen {generation}" }
                }
            }
        }
        if generation.is_multiple_of(2) {
            footer { "footer {generation}" }
        }
    }
}

#[test]
fn keyed_list_grow_shrink() {
    let mut harness = Harness::new(list_app);
    for _ in 0..300 {
        harness.step();
    }
}
