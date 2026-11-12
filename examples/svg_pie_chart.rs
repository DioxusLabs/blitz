//! An animated donut chart drawn with native inline SVG (`svg-native`).
//!
//! Every frame of a data transition rewrites the slices' `d` attributes from
//! Rust, while hover effects are plain CSS transitions on the SVG elements
//! (`transform` and `opacity`), hit-tested against the slice geometry.
use dioxus_native::prelude::*;
use dioxus_native::winit::dpi::LogicalSize;
use dioxus_native::winit::window::WindowAttributes;
use std::f64::consts::{FRAC_PI_2, TAU};
use std::time::Duration;

fn main() {
    // The animation loop uses tokio timers.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = rt.enter();

    let window = WindowAttributes::default()
        .with_title("Native SVG pie chart")
        .with_surface_size(LogicalSize::new(920, 580));
    let config = dioxus_native::Config::new()
        .with_window_attributes(window)
        .with_native_svg(true);
    dioxus_native::launch_cfg(app, vec![], vec![Box::new(config)]);
}

const CATEGORIES: [(&str, &str); 6] = [
    ("Rendering", "#4f7cff"),
    ("Layout", "#22b8a7"),
    ("Style", "#f2b134"),
    ("Text", "#ef6461"),
    ("Network", "#9b6ef3"),
    ("Scripting", "#5bc0eb"),
];

const OUTER_RADIUS: f64 = 150.0;
const INNER_RADIUS: f64 = 84.0;
/// How far a hovered slice moves out from the centre, in user units.
const POP_DISTANCE: f64 = 14.0;

fn app() -> Element {
    let mut targets = use_signal(random_values);
    let mut values = use_signal(|| targets.peek().clone());
    // Fraction of the full circle drawn, animated from 0 on startup.
    let mut reveal = use_signal(|| 0.0_f64);
    let mut hovered = use_signal(|| None::<usize>);
    let mut autoplay = use_signal(|| false);

    // Ease the drawn values towards their targets, one step per frame. Only
    // writes (and so re-renders) while something is still moving.
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_millis(16)).await;
            let r = *reveal.peek();
            if r < 1.0 {
                reveal.set((r + 0.025).min(1.0));
            }
            let goal = targets.peek().clone();
            let current = values.peek().clone();
            if current.iter().zip(&goal).any(|(c, g)| (c - g).abs() > 1e-3) {
                let next = current
                    .iter()
                    .zip(&goal)
                    .map(|(c, g)| c + (g - c) * 0.12)
                    .collect();
                values.set(next);
            }
        }
    });

    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_millis(1600)).await;
            if *autoplay.peek() {
                targets.set(random_values());
            }
        }
    });

    let values = values.read();
    let total: f64 = values.iter().sum();
    let sweep = TAU * ease_out_cubic(reveal());
    let percent = |v: f64| if total > 0.0 { v / total * 100.0 } else { 0.0 };

    let mut start = -FRAC_PI_2;
    let slices = values.iter().enumerate().map(|(i, &value)| {
        let angle = if total > 0.0 {
            value / total * sweep
        } else {
            0.0
        };
        let (a0, a1) = (start, start + angle);
        start = a1;
        let mid = (a0 + a1) / 2.0;
        let (name, color) = CATEGORIES[i];
        let is_hovered = hovered() == Some(i);
        let dimmed = hovered().is_some() && !is_hovered;
        let (dx, dy) = if is_hovered {
            (mid.cos() * POP_DISTANCE, mid.sin() * POP_DISTANCE)
        } else {
            (0.0, 0.0)
        };
        let class = if dimmed { "slice dimmed" } else { "slice" };
        rsx! {
            g {
                key: "{name}",
                class,
                style: "transform: translate({dx:.2}px, {dy:.2}px)",
                onmouseenter: move |_| hovered.set(Some(i)),
                onmouseleave: move |_| hovered.set(None),
                path { d: donut_slice(a0, a1), fill: color }
            }
        }
    });

    let (label, detail) = match hovered() {
        Some(i) => (
            format!("{:.1}%", percent(values[i])),
            CATEGORIES[i].0.to_string(),
        ),
        None => (format!("{total:.0}"), "total ms".to_string()),
    };

    let legend = CATEGORIES.iter().enumerate().map(|(i, &(name, color))| {
        let class = if hovered() == Some(i) {
            "legend-row active"
        } else {
            "legend-row"
        };
        let share = percent(values[i]);
        rsx! {
            div {
                key: "{name}",
                class,
                onmouseenter: move |_| hovered.set(Some(i)),
                onmouseleave: move |_| hovered.set(None),
                div { class: "swatch", style: "background: {color}" }
                div { class: "legend-name", "{name}" }
                div { class: "legend-value", "{share:.1}%" }
                div { class: "bar",
                    div { class: "bar-fill", style: "width: {share:.2}%; background: {color}" }
                }
            }
        }
    });

    rsx! {
        style { {STYLES} }
        div { class: "page",
            h1 { "Frame time breakdown" }
            p { class: "subtitle",
                "Native inline SVG: slices are re-shaped from Rust every frame, hover effects are CSS transitions."
            }
            div { class: "content",
                svg {
                    class: "chart",
                    width: "400",
                    height: "400",
                    "viewBox": "-200 -200 400 400",
                    g { class: "slices", {slices} }
                    text { class: "center-value", x: "0", y: "8", "text-anchor": "middle", "{label}" }
                    text { class: "center-label", x: "0", y: "36", "text-anchor": "middle", "{detail}" }
                }
                div { class: "side",
                    div { class: "legend", {legend} }
                    div { class: "buttons",
                        button { onclick: move |_| targets.set(random_values()), "Randomize" }
                        button {
                            class: if autoplay() { "active" } else { "" },
                            onclick: move |_| autoplay.toggle(),
                            if autoplay() { "Stop auto-play" } else { "Auto-play" }
                        }
                        button { onclick: move |_| reveal.set(0.0), "Replay intro" }
                    }
                }
            }
        }
    }
}

/// The path of a donut slice from angle `a0` to `a1` (radians, clockwise
/// from the positive x axis, as SVG's y axis points down).
fn donut_slice(a0: f64, a1: f64) -> String {
    // A full-circle arc has coincident end points and draws nothing, so stop
    // just short of it.
    let a1 = a1.min(a0 + TAU - 1e-4);
    let large_arc = u8::from(a1 - a0 > std::f64::consts::PI);
    let point = |r: f64, a: f64| (r * a.cos(), r * a.sin());
    let (ox0, oy0) = point(OUTER_RADIUS, a0);
    let (ox1, oy1) = point(OUTER_RADIUS, a1);
    let (ix1, iy1) = point(INNER_RADIUS, a1);
    let (ix0, iy0) = point(INNER_RADIUS, a0);
    format!(
        "M {ox0:.3} {oy0:.3} A {OUTER_RADIUS} {OUTER_RADIUS} 0 {large_arc} 1 {ox1:.3} {oy1:.3} \
         L {ix1:.3} {iy1:.3} A {INNER_RADIUS} {INNER_RADIUS} 0 {large_arc} 0 {ix0:.3} {iy0:.3} Z"
    )
}

fn random_values() -> Vec<f64> {
    CATEGORIES
        .iter()
        .map(|_| rand::random_range(2.0..40.0_f64).round())
        .collect()
}

fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

static STYLES: &str = r#"
    body {
        margin: 0;
        font-family: sans-serif;
        background: #f5f6fa;
        color: #1f2430;
    }
    .page { padding: 28px 36px; }
    h1 { margin: 0 0 4px; font-size: 26px; }
    .subtitle { margin: 0 0 20px; color: #6b7280; font-size: 14px; }
    .content { display: flex; gap: 40px; align-items: center; }

    .chart { flex-shrink: 0; }
    .slice {
        cursor: pointer;
        transition: transform 0.25s ease-out, opacity 0.25s ease-out;
    }
    .slice path { stroke: #f5f6fa; stroke-width: 3; }
    .slice.dimmed { opacity: 0.35; }
    .center-value { font-size: 34px; font-weight: bold; fill: #1f2430; }
    .center-label { font-size: 15px; fill: #6b7280; }

    .side { display: flex; flex-direction: column; gap: 20px; width: 380px; }
    .legend { display: flex; flex-direction: column; gap: 4px; }
    .legend-row {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 4px 10px;
        padding: 6px 10px;
        border-radius: 8px;
        cursor: pointer;
        transition: background-color 0.2s;
    }
    .legend-row.active { background: #e3e7f2; }
    .swatch { width: 14px; height: 14px; border-radius: 4px; }
    .legend-name { flex-grow: 1; font-size: 15px; }
    .legend-value { width: 56px; text-align: right; font-variant-numeric: tabular-nums; font-size: 15px; }
    .bar { width: 100%; height: 4px; border-radius: 2px; background: #e3e7f2; }
    .bar-fill { height: 4px; border-radius: 2px; }

    .buttons { display: flex; gap: 8px; }
    button {
        padding: 8px 14px;
        border: 1px solid #c7cdd9;
        border-radius: 8px;
        background: white;
        font-size: 14px;
        cursor: pointer;
        white-space: nowrap;
        transition: background-color 0.15s, color 0.15s;
    }
    button:hover { background: #e3e7f2; }
    button.active { background: #4f7cff; border-color: #4f7cff; color: white; }
"#;
