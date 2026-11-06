//! Benchmark building AccessKit tree updates: the full tree, incremental updates after
//! typical, layout-only and large changes, and per-node bounds via
//! `get_client_bounding_rect` for comparison.
//!
//! Usage: accessibility_bench [html file] [iters]
//! Without a file, a generated page with a 10,000-row table is used.

use blitz_dom::{BaseDocument, DocumentConfig, NodeId};
use blitz_html::HtmlDocument;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::collections::HashSet;
use std::time::{Duration, Instant};

fn median(mut times: Vec<Duration>) -> Duration {
    times.sort_unstable();
    times[times.len() / 2]
}

fn bench(iters: usize, mut f: impl FnMut() -> Duration) -> Duration {
    for _ in 0..3 {
        f();
    }
    median((0..iters).map(|_| f()).collect())
}

fn timed<T>(f: impl FnOnce() -> T) -> (Duration, T) {
    let start = Instant::now();
    let out = f();
    (start.elapsed(), out)
}

fn report(label: &str, time: Duration, nodes: usize) {
    println!(
        "{label:<44} {:>9.3} ms  {nodes:>7} nodes",
        time.as_secs_f64() * 1000.0
    );
}

fn generated_page() -> String {
    let mut html = String::from(
        "<html><body><header><h1>Rows</h1><button id=toggle aria-expanded=false>Menu</button>\
         <span id=counter>0</span></header><table><tbody id=rows>",
    );
    for i in 0..10_000 {
        html.push_str(&format!(
            "<tr><td>{i}</td><td><a href=#>Row {i}</a></td><td><button>Delete</button></td></tr>"
        ));
    }
    html.push_str("</tbody></table></body></html>");
    html
}

/// Resolve, then time building the incremental update
fn update(doc: &mut BaseDocument) -> (Duration, usize) {
    doc.resolve(0.0);
    let (time, update) = timed(|| doc.build_accessibility_tree_update());
    doc.clear_changes();
    (time, update.nodes.len())
}

/// The body child with the most descendants
fn largest_section(doc: &BaseDocument) -> NodeId {
    let body = doc.query_selector("body").unwrap().unwrap();
    let count = |id: NodeId| {
        let mut count = 0;
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            count += 1;
            stack.extend(doc.get_node(id).unwrap().children.iter().copied());
        }
        count
    };
    let children = doc.get_node(body).unwrap().children.clone();
    children
        .into_iter()
        .filter(|&id| doc.get_node(id).unwrap().is_element())
        .max_by_key(|&id| count(id))
        .unwrap()
}

fn first_text_in(doc: &BaseDocument, id: NodeId) -> NodeId {
    let mut stack = vec![id];
    while let Some(id) = stack.pop() {
        let node = doc.get_node(id).unwrap();
        if node.is_text_node() && !node.text_content().trim().is_empty() {
            return id;
        }
        stack.extend(node.children.iter().rev().copied());
    }
    panic!("no text");
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next();
    let iters: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(20);
    let html = match &path {
        Some(path) => std::fs::read_to_string(path).unwrap(),
        None => generated_page(),
    };

    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(2732, 1536, 2.0, ColorScheme::Light)),
            ..Default::default()
        },
    );
    let doc: &mut BaseDocument = &mut doc;
    doc.resolve(0.0);
    println!(
        "{}: {} DOM nodes",
        path.as_deref().unwrap_or("generated 10,000-row table"),
        doc.tree().len()
    );

    // Full tree
    let mut tree_size = 0;
    let full = bench(iters, || {
        let (time, tree) = timed(|| doc.build_accessibility_tree());
        tree_size = tree.nodes.len();
        time
    });
    report("full tree", full, tree_size);
    doc.clear_changes();

    report("update, nothing changed", update(doc).0, update(doc).1);

    // A typical interaction: change one text node
    let section = largest_section(doc);
    let text = first_text_in(doc, section);
    let mut nodes = 0;
    let mut i = 0;
    let time = bench(iters, || {
        i += 1;
        doc.mutate().set_node_text(text, &format!("Text {i}"));
        let (time, count) = update(doc);
        nodes = count;
        time
    });
    report("update, text change", time, nodes);

    // Another: toggle an attribute
    let element = doc.get_node(text).unwrap().parent.unwrap();
    let time = bench(iters, || {
        i += 1;
        let value = if i % 2 == 0 { "true" } else { "false" };
        doc.mutate()
            .set_attribute(element, blitz_dom::qual_name!("aria-expanded"), value);
        let (time, count) = update(doc);
        nodes = count;
        time
    });
    report("update, attribute change", time, nodes);

    // Layout-only changes: content after a growing element moves
    let first = doc
        .get_node(doc.query_selector("body").unwrap().unwrap())
        .unwrap()
        .children
        .iter()
        .copied()
        .find(|&id| doc.get_node(id).unwrap().is_element())
        .unwrap();
    let time = bench(iters, || {
        i += 1;
        let height = format!("{}px", 100 + i % 2 * 10);
        doc.mutate().set_style_property(first, "height", &height);
        let (time, count) = update(doc);
        nodes = count;
        time
    });
    report("update, first section height change", time, nodes);

    let time = bench(iters, || {
        i += 1;
        doc.set_viewport_scroll(blitz_dom::Point {
            x: 0.0,
            y: (i % 2 * 100) as f64,
        });
        let (time, count) = update(doc);
        nodes = count;
        time
    });
    report("update, page scroll", time, nodes);

    let time = bench(iters.min(10), || {
        i += 1;
        let width = 2732 - (i % 2) as u32 * 400;
        doc.set_viewport(Viewport::new(width, 1536, 2.0, ColorScheme::Light));
        let (time, count) = update(doc);
        nodes = count;
        time
    });
    report("update, window resize", time, nodes);

    // A large change: replace the largest section of the page with a copy of itself
    let mut section = section;
    let mut mutate_time = Duration::ZERO;
    let time = bench(iters.min(10), || {
        let (time, clone) = timed(|| {
            let mut m = doc.mutate();
            let clone = m.deep_clone_node(section);
            m.replace_node_with(section, &[clone]);
            m.remove_node_if_unparented(section);
            clone
        });
        mutate_time = time;
        section = clone;
        let (time, count) = update(doc);
        nodes = count;
        time
    });
    report("update, replace largest section", time, nodes);
    let (time, tree) = timed(|| doc.build_accessibility_tree());
    report("full tree, after replacing", time, tree.nodes.len());
    doc.clear_changes();

    // Hide and show the largest section
    let time = bench(iters.min(10), || {
        doc.mutate().set_style_property(section, "display", "none");
        update(doc);
        doc.mutate().remove_style_property(section, "display");
        let (time, count) = update(doc);
        nodes = count;
        time
    });
    report("update, show largest section", time, nodes);

    // Cost of recording changes: the large mutation with and without tracking
    let mut replace = |doc: &mut BaseDocument| {
        let (time, clone) = timed(|| {
            let mut m = doc.mutate();
            let clone = m.deep_clone_node(section);
            m.replace_node_with(section, &[clone]);
            m.remove_node_if_unparented(section);
            clone
        });
        section = clone;
        let (resolve, _) = timed(|| doc.resolve(0.0));
        doc.clear_changes();
        time + resolve
    };
    let tracking = bench(iters.min(10), || replace(doc));
    doc.stop_accessibility_tracking();
    let untracked = bench(iters.min(10), || replace(doc));
    report("replace section + resolve, tracking", tracking, 0);
    report("replace section + resolve, not tracking", untracked, 0);

    // Bounds: get_client_bounding_rect for every node in the tree (text nodes use
    // their parent's rect), as in DioxusLabs/blitz#842
    doc.resolve(0.0);
    let tree = doc.build_accessibility_tree();
    let ids: HashSet<u64> = tree.nodes.iter().map(|(id, _)| id.0).collect();
    let mut in_tree = Vec::new();
    doc.visit(|id, node| {
        if ids.contains(&(id.as_u64())) && (node.is_element() || node.is_text_node()) {
            in_tree.push(match node.is_text_node() {
                true => node.parent.unwrap(),
                false => id,
            });
        }
    });
    let time = bench(iters, || {
        timed(|| {
            for &id in &in_tree {
                std::hint::black_box(doc.get_client_bounding_rect(id));
            }
        })
        .0
    });
    report(
        "get_client_bounding_rect for every node",
        time,
        in_tree.len(),
    );
}
