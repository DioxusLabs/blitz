//! Data-driven interaction scenarios: regression tests for interactive behavior
//! expressed as JSON fixture files rather than Rust code.
//!
//! A scenario is a JSON document with initial `html` (plus optional `viewport`,
//! `base_url`, and `fixture_dir`) and a list of `steps` — interactions and assertions
//! executed in order against a [`Harness`]:
//!
//! ```json
//! {
//!     "html": "<input id='inp'><div id='out'>ready</div>",
//!     "steps": [
//!         { "step": "click", "selector": "#inp" },
//!         { "step": "type", "text": "hello" },
//!         { "step": "assert_value", "selector": "#inp", "equals": "hello" },
//!         { "step": "assert_text", "selector": "#out", "equals": "ready" }
//!     ]
//! }
//! ```

use std::path::Path;
use std::sync::Arc;

use serde::Deserialize;

use crate::{FileNetProvider, Harness, HarnessOptions, parse_key};

#[derive(Debug, Deserialize)]
pub struct Scenario {
    /// Initial HTML for the document
    pub html: String,
    /// Viewport size as `[width, height]` (defaults to 800x600)
    #[serde(default)]
    pub viewport: Option<[u32; 2]>,
    /// Base URL for resolving relative sub-resource URLs
    #[serde(default)]
    pub base_url: Option<String>,
    /// Directory to serve sub-resources from (via [`FileNetProvider`])
    #[serde(default)]
    pub fixture_dir: Option<String>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum Step {
    /// Click the element matching `selector`
    Click { selector: String },
    /// Move the mouse to `(x, y)`
    Move { x: f32, y: f32 },
    /// Type text via key events (into the focused element)
    Type { text: String },
    /// Press and release a key (e.g. "Enter", "Tab", or a single character)
    Key { key: String },
    /// Dispatch a wheel event at `(x, y)`
    Scroll {
        x: f32,
        y: f32,
        #[serde(default)]
        delta_x: f64,
        #[serde(default)]
        delta_y: f64,
    },
    /// Advance the animation clock by `seconds`
    Tick { seconds: f64 },
    /// Assert the text content of the element matching `selector`
    AssertText { selector: String, equals: String },
    /// Assert the current value of the text input matching `selector`
    AssertValue { selector: String, equals: String },
    /// Assert (a subset of) the layout rect of the element matching `selector`
    AssertLayout {
        selector: String,
        #[serde(default)]
        x: Option<f32>,
        #[serde(default)]
        y: Option<f32>,
        #[serde(default)]
        width: Option<f32>,
        #[serde(default)]
        height: Option<f32>,
    },
    /// Assert that the element matching `selector` is focused
    AssertFocused { selector: String },
    /// Assert the rendered output matches a reference PNG
    /// (path resolved relative to the scenario file, if any)
    AssertScreenshot { reference: String },
}

/// Run a scenario from a JSON file. Panics (with the failing step) on assertion failure.
pub fn run_scenario_file(path: impl AsRef<Path>) {
    let path = path.as_ref();
    let json = std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read scenario {}: {err}", path.display()));
    let scenario: Scenario = serde_json::from_str(&json)
        .unwrap_or_else(|err| panic!("failed to parse scenario {}: {err}", path.display()));
    run_scenario(&scenario, path.parent());
}

/// Run an already-parsed scenario. Relative reference/fixture paths are resolved
/// against `base_dir` when provided.
pub fn run_scenario(scenario: &Scenario, base_dir: Option<&Path>) {
    let resolve = |p: &str| -> std::path::PathBuf {
        let p = Path::new(p);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            base_dir.map(|dir| dir.join(p)).unwrap_or_else(|| p.into())
        }
    };

    let (width, height) = match scenario.viewport {
        Some([w, h]) => (w, h),
        None => (800, 600),
    };
    let options = HarnessOptions {
        width,
        height,
        base_url: scenario.base_url.clone(),
        net_provider: scenario
            .fixture_dir
            .as_deref()
            .map(|dir| Arc::new(FileNetProvider::new(resolve(dir))) as _),
        ..Default::default()
    };
    let mut harness = Harness::from_html_with(&scenario.html, options);

    for (i, step) in scenario.steps.iter().enumerate() {
        let step_context = format!("step {i} ({step:?})");
        match step {
            Step::Click { selector } => harness.click(selector),
            Step::Move { x, y } => harness.move_mouse_to(*x, *y),
            Step::Type { text } => harness.type_text(text),
            Step::Key { key } => harness.press(parse_key(key)),
            Step::Scroll {
                x,
                y,
                delta_x,
                delta_y,
            } => harness.wheel_at(*x, *y, *delta_x, *delta_y),
            Step::Tick { seconds } => harness.tick(*seconds),
            Step::AssertText { selector, equals } => {
                let actual = harness.text_content(selector);
                assert_eq!(&actual, equals, "{step_context}: text mismatch");
            }
            Step::AssertValue { selector, equals } => {
                let actual = harness.input_value(selector);
                assert_eq!(
                    actual.as_deref(),
                    Some(equals.as_str()),
                    "{step_context}: value mismatch"
                );
            }
            Step::AssertLayout {
                selector,
                x,
                y,
                width,
                height,
            } => {
                let rect = harness.layout_rect(selector);
                let checks = [
                    (x, rect.x),
                    (y, rect.y),
                    (width, rect.width),
                    (height, rect.height),
                ];
                for (expected, actual) in checks {
                    if let Some(expected) = expected {
                        assert_eq!(*expected, actual, "{step_context}: layout mismatch");
                    }
                }
            }
            Step::AssertFocused { selector } => {
                let node = harness.node(selector);
                assert_eq!(
                    harness.focused(),
                    Some(node),
                    "{step_context}: focus mismatch"
                );
            }
            Step::AssertScreenshot { reference } => {
                harness.assert_screenshot_matches(resolve(reference));
            }
        }
    }
}
