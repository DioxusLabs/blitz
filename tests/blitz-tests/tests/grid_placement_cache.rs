//! Caching of grid item placement: taffy runs the placement algorithm once
//! for a grid container and stores the result on the node, reusing it every
//! other time that the container is sized or laid out. The stored result is
//! discarded along with the node's layout cache, so it is recomputed whenever
//! the container (or anything in it) is damaged.
//!
//! In debug builds taffy checks every cached placement that it uses against
//! the result of running the placement algorithm (and panics if they differ),
//! so every mutation below also checks that the cache was invalidated.

use blitz_dom::{DocumentConfig, QualName, ns};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::node_id::NodeId;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn attr(name: &str) -> QualName {
    QualName::new(None, ns!(), name.into())
}

fn make_doc(html: &str, incremental: bool) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 400, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.set_incremental_layout(incremental);
    doc.resolve(0.0);
    doc
}

fn id(doc: &HtmlDocument, selector: &str) -> NodeId {
    doc.query_selector(selector).unwrap().unwrap()
}

/// The (x, y) location of each of the passed elements
fn locations<const N: usize>(doc: &HtmlDocument, selectors: [&str; N]) -> [(f32, f32); N] {
    selectors.map(|selector| {
        let location = doc
            .get_node(id(doc, selector))
            .unwrap()
            .final_layout()
            .location;
        (location.x, location.y)
    })
}

fn has_cached_placement(doc: &HtmlDocument, selector: &str) -> bool {
    let node = doc.get_node(id(doc, selector)).unwrap();
    node.element_data().unwrap().grid_placement_cache.is_some()
}

/// The number of times that taffy has run the grid item placement algorithm
/// on this thread (which it only tracks in debug builds)
fn placement_runs() -> Option<usize> {
    #[cfg(debug_assertions)]
    return Some(taffy::compute::grid_placement_runs());
    #[cfg(not(debug_assertions))]
    return None;
}

/// `#grid` has three auto-sized columns, and is the only item of `#outer`,
/// whose auto-sized column makes it measure `#grid` more than once per layout.
const HTML: &str = r#"<html><body style="margin:0">
    <div id="outer" style="display:grid; grid-template-columns:auto; justify-items:start;">
        <div id="grid" style="display:grid; grid-template-columns:auto auto auto;">
            <div id="a" style="width:10px; height:10px;"></div>
            <div id="b" style="width:20px; height:10px;"></div>
            <div id="c" style="width:30px; height:10px;"></div>
            <div id="d" style="width:40px; height:10px;"></div>
            <div id="e" style="width:50px; height:10px;"></div>
        </div>
    </div>
</body></html>"#;

const ITEMS: [&str; 5] = ["#a", "#b", "#c", "#d", "#e"];

#[test]
fn placement_runs_once_per_grid_container() {
    for incremental in [false, true] {
        let before = placement_runs();
        let doc = make_doc(HTML, incremental);

        assert_eq!(
            locations(&doc, ITEMS),
            [
                (0.0, 0.0),
                (40.0, 0.0),
                (90.0, 0.0),
                (0.0, 10.0),
                (40.0, 10.0)
            ],
            "incremental={incremental}"
        );
        assert!(has_cached_placement(&doc, "#outer"));
        assert!(has_cached_placement(&doc, "#grid"));
        if let (Some(before), Some(after)) = (before, placement_runs()) {
            assert_eq!(after - before, 2, "incremental={incremental}");
        }
    }
}

#[test]
fn placement_is_reused_when_nothing_is_damaged() {
    let mut doc = make_doc(HTML, true);

    let before = placement_runs();
    doc.resolve(0.0);
    assert_eq!(placement_runs(), before);
    assert!(has_cached_placement(&doc, "#grid"));
}

/// Apply `mutate` to a laid out document, lay it out again, and check the new
/// locations of the grid's original items.
#[track_caller]
fn check_mutation(mutate: impl Fn(&mut HtmlDocument), expected: [(f32, f32); 5]) {
    for incremental in [false, true] {
        let mut doc = make_doc(HTML, incremental);
        mutate(&mut doc);
        doc.resolve(0.0);
        assert_eq!(
            locations(&doc, ITEMS),
            expected,
            "incremental={incremental}"
        );
    }
}

fn set_style(doc: &mut HtmlDocument, selector: &str, style: &str) {
    let node_id = id(doc, selector);
    doc.mutate().set_attribute(node_id, attr("style"), style);
}

#[test]
fn changing_an_items_grid_placement_invalidates_placement() {
    check_mutation(
        |doc| {
            set_style(
                doc,
                "#b",
                "width:20px; height:10px; grid-column:3; grid-row:2;",
            )
        },
        // Row 1 is now a, c, d and row 2 is e, (empty), b
        [
            (0.0, 0.0),
            (80.0, 10.0),
            (50.0, 0.0),
            (80.0, 0.0),
            (0.0, 10.0),
        ],
    );
}

#[test]
fn hiding_an_item_invalidates_placement() {
    check_mutation(
        |doc| set_style(doc, "#b", "display:none"),
        // Row 1 is now a, c, d and row 2 is e
        [
            (0.0, 0.0),
            (0.0, 0.0),
            (50.0, 0.0),
            (80.0, 0.0),
            (0.0, 10.0),
        ],
    );
}

#[test]
fn taking_an_item_out_of_flow_invalidates_placement() {
    check_mutation(
        |doc| set_style(doc, "#a", "width:10px; height:10px; position:absolute;"),
        // Row 1 is now b, c, d and row 2 is e
        [
            (0.0, 0.0),
            (0.0, 0.0),
            (50.0, 0.0),
            (80.0, 0.0),
            (0.0, 10.0),
        ],
    );
}

#[test]
fn changing_an_items_order_invalidates_placement() {
    check_mutation(
        |doc| set_style(doc, "#e", "width:50px; height:10px; order:-1;"),
        [
            (50.0, 0.0),
            (90.0, 0.0),
            (0.0, 10.0),
            (50.0, 10.0),
            (0.0, 0.0),
        ],
    );
}

#[test]
fn removing_an_item_invalidates_placement() {
    for incremental in [false, true] {
        let mut doc = make_doc(HTML, incremental);
        let node_id = id(&doc, "#a");
        doc.mutate().remove_node(node_id);
        doc.resolve(0.0);
        // Row 1 is now b, c, d and row 2 is e
        assert_eq!(
            locations(&doc, ["#b", "#c", "#d", "#e"]),
            [(0.0, 0.0), (50.0, 0.0), (80.0, 0.0), (0.0, 10.0)],
            "incremental={incremental}"
        );
    }
}

#[test]
fn changing_the_container_template_invalidates_placement() {
    check_mutation(
        |doc| {
            set_style(
                doc,
                "#grid",
                "display:grid; grid-template-columns:auto auto;",
            )
        },
        [
            (0.0, 0.0),
            (50.0, 0.0),
            (0.0, 10.0),
            (50.0, 10.0),
            (0.0, 20.0),
        ],
    );
}

#[test]
fn changing_the_container_auto_flow_invalidates_placement() {
    check_mutation(
        |doc| {
            let style = "display:grid; grid-template-rows:auto auto; grid-auto-flow:column;";
            set_style(doc, "#grid", style);
        },
        [
            (0.0, 0.0),
            (0.0, 10.0),
            (20.0, 0.0),
            (20.0, 10.0),
            (60.0, 0.0),
        ],
    );
}

/// Resizing an item changes the layout but not the placement. The stored
/// placement is still discarded (it shares the lifetime of the layout cache),
/// but placement runs only once for each damaged grid container.
#[test]
fn resizing_an_item_runs_placement_once_per_damaged_grid_container() {
    let mut doc = make_doc(HTML, true);
    set_style(&mut doc, "#a", "width:25px; height:10px;");

    let before = placement_runs();
    doc.resolve(0.0);
    if let (Some(before), Some(after)) = (before, placement_runs()) {
        assert_eq!(after - before, 2);
    }
    assert_eq!(
        locations(&doc, ITEMS),
        [
            (0.0, 0.0),
            (40.0, 0.0),
            (90.0, 0.0),
            (0.0, 10.0),
            (40.0, 10.0)
        ],
    );
}
