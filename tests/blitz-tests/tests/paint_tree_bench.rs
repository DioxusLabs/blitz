//! Timing harness for the paint tree pass and hoisted-geometry lookups.
//!
//! Run with: `cargo test -p blitz-tests --release --test paint_tree_bench -- --ignored --nocapture`

use anyrender::NullScenePainter;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

const WIDTH: u32 = 1200;
const HEIGHT: u32 = 800;

fn make_doc(html: &str, incremental: bool) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(WIDTH, HEIGHT, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            base_url: Some("https://example.com/".to_string()),
            incremental: Some(incremental),
            ..Default::default()
        },
    );
    // Two frames: on `main` hoisted positions are one frame behind layout.
    doc.resolve(0.0);
    doc.resolve(0.0);
    doc
}

fn nest(depth: usize, inner: &str) -> String {
    let mut s = String::new();
    for _ in 0..depth {
        s.push_str("<div class=\"n\">");
    }
    s.push_str(inner);
    for _ in 0..depth {
        s.push_str("</div>");
    }
    s
}

const STYLE: &str = r#"
    body { margin: 0; }
    .n { padding: 0; }
    .z { position: relative; z-index: 1; height: 2px; width: 50px; background: red; }
    .card { height: 4px; width: 200px; background: #eee; }
    .sc { opacity: 0.99; }
    #hover { height: 2px; width: 20px; background: blue; }
    #hover:hover { background: green; }
"#;

fn page(style: &str, body: &str) -> String {
    format!("<html><head><style>{STYLE}{style}</style></head><body>{body}</body></html>")
}

/// ~2000 nodes: 100 cards each nested ~15 deep, 20 of which hold a z-indexed box.
fn realistic_page() -> String {
    let mut body = String::new();
    for i in 0..100 {
        let inner = if i % 5 == 0 {
            "<div class=\"card\"><div class=\"z\"></div></div>"
        } else {
            "<div class=\"card\"></div>"
        };
        body.push_str(&nest(14, inner));
    }
    body.push_str(&nest(14, "<div id=\"hover\"></div>"));
    page("", &body)
}

/// 5000 `position: relative; z-index: 1` items at depth ~20 under the root SC.
fn stress_page() -> String {
    let mut items = String::new();
    for _ in 0..5000 {
        items.push_str("<div class=\"z\"></div>");
    }
    items.push_str("<div id=\"hover\"></div>");
    page("", &nest(19, &items))
}

/// Same 5000 items, split across 50 nested SC roots (100 items each).
fn nested_sc_page() -> String {
    let mut body = String::new();
    for _ in 0..50 {
        body.push_str("<div class=\"sc\">");
        for _ in 0..100 {
            body.push_str("<div class=\"z\"></div>");
        }
    }
    body.push_str("<div id=\"hover\"></div>");
    for _ in 0..50 {
        body.push_str("</div>");
    }
    page("", &nest(19, &body))
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

fn time_median(rounds: usize, mut f: impl FnMut()) -> Duration {
    let samples = (0..rounds)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed()
        })
        .collect();
    median(samples)
}

fn fmt(d: Duration) -> String {
    let ns = d.as_nanos();
    if ns < 10_000 {
        format!("{ns} ns")
    } else if ns < 10_000_000 {
        format!("{:.1} µs", ns as f64 / 1e3)
    } else {
        format!("{:.2} ms", ns as f64 / 1e6)
    }
}

struct Row {
    page: &'static str,
    nodes: usize,
    hit: Duration,
    render: Duration,
    hover_frame: Duration,
    full_frame: Duration,
}

fn bench_page(name: &'static str, html: &str) -> Row {
    let mut doc = make_doc(html, true);
    let nodes = doc.tree().len();

    // Hit-test a point over the first z-indexed box (worst case: hit walks
    // hoisted children in reverse order, so every entry is tested).
    let first_z = doc.query_selector(".z").unwrap().unwrap();
    let mut y = 0.0;
    let mut x = 0.0;
    let mut current = Some(first_z);
    while let Some(id) = current {
        let node = doc.get_node(id).unwrap();
        x += node.final_layout().location.x;
        y += node.final_layout().location.y;
        current = node.layout_parent.get();
    }
    let (hx, hy) = (x + 1.0, y + 1.0);
    assert_eq!(doc.hit(hx, hy).map(|h| h.node_id), Some(first_z));

    const HITS: u32 = 200;
    let hit = time_median(100, || {
        for _ in 0..HITS {
            std::hint::black_box(doc.hit(hx, hy));
        }
    }) / HITS;

    let mut scene = NullScenePainter;
    let render = time_median(50, || {
        blitz_paint::paint_scene(&mut scene, &mut doc, 1.0, WIDTH, HEIGHT, 0, 0);
    });

    // Hover-only incremental frame: alternate hovering `#hover` (colour
    // change) and nothing.
    let hover_id = doc.query_selector("#hover").unwrap().unwrap();
    let mut hy2 = 0.0;
    let mut hx2 = 0.0;
    let mut current = Some(hover_id);
    while let Some(id) = current {
        let node = doc.get_node(id).unwrap();
        hx2 += node.final_layout().location.x;
        hy2 += node.final_layout().location.y;
        current = node.layout_parent.get();
    }
    let mut on = false;
    let hover_frame = time_median(100, || {
        on = !on;
        if on {
            doc.set_hover_to(hx2 + 1.0, hy2 + 1.0);
        } else {
            doc.set_hover_to(WIDTH as f32 - 1.0, HEIGHT as f32 - 1.0);
        }
        doc.resolve(0.0);
    });

    let mut doc = make_doc(html, false);
    let full_frame = time_median(30, || {
        doc.set_hover_to(hx2 + 1.0, hy2 + 1.0);
        doc.resolve(0.0);
        doc.set_hover_to(WIDTH as f32 - 1.0, HEIGHT as f32 - 1.0);
        doc.resolve(0.0);
    }) / 2;

    Row {
        page: name,
        nodes,
        hit,
        render,
        hover_frame,
        full_frame,
    }
}

/// ~40k nodes: 2000 cards of ~20 nodes with some z-indexed boxes.
fn large_page() -> String {
    let mut body = String::new();
    for i in 0..2000 {
        let inner = if i % 10 == 0 {
            "<div class=\"card\"><span>a</span><div class=\"z\"></div><span>b</span></div>"
        } else {
            "<div class=\"card\"><span>a</span><span>b</span></div>"
        };
        body.push_str(&nest(16, inner));
    }
    body.push_str(&nest(16, "<div id=\"hover\"></div>"));
    page("", &body)
}

/// Prints per-phase resolve timings (needs `--features blitz-dom/log-phase-times`)
/// for full non-incremental frames and for hover-only incremental frames.
#[test]
#[ignore]
fn resolve_phase_timings() {
    let html = large_page();
    let mut doc = make_doc(&html, false);
    println!("nodes: {}", doc.tree().len());
    println!("--- non-incremental frames");
    for _ in 0..5 {
        doc.resolve(0.0);
    }

    let mut doc = make_doc(&html, true);
    let hover_id = doc.query_selector("#hover").unwrap().unwrap();
    let mut hy = 0.0;
    let mut hx = 0.0;
    let mut current = Some(hover_id);
    while let Some(id) = current {
        let node = doc.get_node(id).unwrap();
        hx += node.final_layout().location.x;
        hy += node.final_layout().location.y;
        current = node.layout_parent.get();
    }
    println!("--- incremental hover-only frames");
    for i in 0..6 {
        if i % 2 == 0 {
            doc.set_hover_to(hx + 1.0, hy + 1.0);
        } else {
            doc.set_hover_to(WIDTH as f32 - 1.0, HEIGHT as f32 - 1.0);
        }
        doc.resolve(0.0);
    }
}

/// Same measurements on a real-world page (stylesheets must be inlined):
/// `PAINT_TREE_BENCH_HTML=/path/page.html cargo test ... external_page_timings`.
/// Hit-tests the viewport centre; hovers the first laid-out `<a>`.
/// With `--features blitz-dom/log-phase-times` it also prints per-phase frame timings.
#[test]
#[ignore]
fn external_page_timings() {
    let Ok(path) = std::env::var("PAINT_TREE_BENCH_HTML") else {
        eprintln!("PAINT_TREE_BENCH_HTML not set; skipping");
        return;
    };
    let html = std::fs::read_to_string(path).unwrap();

    let mut doc = make_doc(&html, true);
    let nodes = doc.tree().len();
    let hoisted: usize = doc
        .tree()
        .iter()
        .filter_map(|(_, n)| n.stacking_context.as_ref().map(|sc| sc.children.len()))
        .sum();
    println!("nodes: {nodes}, hoisted SC entries: {hoisted}");

    let (hx, hy) = (WIDTH as f32 / 2.0, HEIGHT as f32 / 2.0);
    const HITS: u32 = 200;
    let hit = time_median(50, || {
        for _ in 0..HITS {
            std::hint::black_box(doc.hit(hx, hy));
        }
    }) / HITS;

    let mut scene = NullScenePainter;
    let render = time_median(20, || {
        blitz_paint::paint_scene(&mut scene, &mut doc, 1.0, WIDTH, HEIGHT, 0, 0);
    });

    let anchor = doc
        .query_selector_all("a")
        .unwrap()
        .into_iter()
        .find(|id| {
            let l = doc.get_node(*id).unwrap().final_layout();
            l.size.width > 0.0 && l.size.height > 0.0
        })
        .unwrap();
    let mut ax = 0.0;
    let mut ay = 0.0;
    let mut current = Some(anchor);
    while let Some(id) = current {
        let node = doc.get_node(id).unwrap();
        ax += node.final_layout().location.x;
        ay += node.final_layout().location.y;
        current = node.layout_parent.get();
    }
    let mut on = false;
    println!("--- incremental hover-only frames");
    let hover_frame = time_median(20, || {
        on = !on;
        if on {
            doc.set_hover_to(ax + 1.0, ay + 1.0);
        } else {
            doc.set_hover_to(WIDTH as f32 - 1.0, HEIGHT as f32 - 1.0);
        }
        doc.resolve(0.0);
    });

    println!("--- non-incremental frames");
    let mut doc = make_doc(&html, false);
    let full_frame = time_median(5, || {
        doc.set_hover_to(ax + 1.0, ay + 1.0);
        doc.resolve(0.0);
        doc.set_hover_to(WIDTH as f32 - 1.0, HEIGHT as f32 - 1.0);
        doc.resolve(0.0);
    }) / 2;

    println!(
        "| external | {nodes} | {} | {} | {} | {} |",
        fmt(hit),
        fmt(render),
        fmt(hover_frame),
        fmt(full_frame)
    );
}

#[test]
#[ignore]
fn paint_tree_timings() {
    let rows = [
        bench_page("realistic", &realistic_page()),
        bench_page("stress (1 SC)", &stress_page()),
        bench_page("stress (50 nested SCs)", &nested_sc_page()),
    ];
    let mut out = String::new();
    writeln!(
        out,
        "| page | nodes | hit() | render (null backend) | hover-only frame (incremental) | full frame (non-incremental) |"
    )
    .unwrap();
    writeln!(out, "|---|---|---|---|---|---|").unwrap();
    for row in rows {
        writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} |",
            row.page,
            row.nodes,
            fmt(row.hit),
            fmt(row.render),
            fmt(row.hover_frame),
            fmt(row.full_frame),
        )
        .unwrap();
    }
    println!("{out}");
}
