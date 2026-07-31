//! Interactive JSON-lines driver for headless Blitz documents.
//!
//! Reads one JSON command per line on stdin and prints one JSON response per line on
//! stdout, letting an agent (or a human with a shell) drive a live document — load HTML,
//! click, type, scroll, inspect layout, and take screenshots — without writing Rust.
//!
//! ```text
//! $ cargo run -p blitz-drive
//! {"cmd":"load_html","html":"<div id=a style='width:50px;height:50px'>hi</div>"}
//! {"ok":true}
//! {"cmd":"layout","selector":"#a"}
//! {"ok":true,"x":8.0,"y":8.0,"width":50.0,"height":50.0}
//! {"cmd":"click","selector":"#a"}
//! {"ok":true}
//! {"cmd":"screenshot","path":"/tmp/shot.png"}
//! {"ok":true,"path":"/tmp/shot.png","width":800,"height":600}
//! ```

use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::Arc;

use blitz_test_harness::{FileNetProvider, Harness, HarnessOptions, parse_key};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum Command {
    /// Load an HTML file from disk. Sub-resources with relative URLs are served from
    /// the file's directory.
    Load { path: String },
    /// Load an HTML string. Optional `fixture_dir` serves sub-resources; optional
    /// `base_url` sets the document base URL.
    LoadHtml {
        html: String,
        base_url: Option<String>,
        fixture_dir: Option<String>,
    },
    /// Click an element (by selector) or a point (by x/y)
    Click {
        selector: Option<String>,
        x: Option<f32>,
        y: Option<f32>,
    },
    /// Move the mouse to a point
    Move { x: f32, y: f32 },
    /// Press and release a key, e.g. "Enter", "Tab", "ArrowDown", or a single character
    Key { key: String },
    /// Type a string of text via key events
    Type { text: String },
    /// Dispatch a wheel event at a point (or the viewport center)
    Scroll {
        x: Option<f32>,
        y: Option<f32>,
        delta_x: Option<f64>,
        delta_y: Option<f64>,
    },
    /// Dump the DOM tree (with layout geometry) as text
    DumpDom,
    /// Dump the document's paint commands as text (one per line)
    DumpPaint,
    /// Get the layout rect of the first element matching `selector`
    Layout { selector: String },
    /// Get the text content of the first element matching `selector`
    Text { selector: String },
    /// Get an attribute of the first element matching `selector`
    Attr { selector: String, name: String },
    /// Get the current value of a text input/textarea matching `selector`
    Value { selector: String },
    /// Count elements matching `selector`
    Query { selector: String },
    /// Hit-test a point, returning the node id (if any)
    Hit { x: f32, y: f32 },
    /// Render the document and write a PNG to `path`
    Screenshot { path: String },
    /// Resize the viewport
    SetViewport { width: u32, height: u32 },
    /// Advance the animation clock by `seconds` and re-resolve
    Tick { seconds: f64 },
}

struct Driver {
    harness: Option<Harness>,
    viewport: (u32, u32),
}

impl Driver {
    fn new() -> Self {
        Self {
            harness: None,
            viewport: (800, 600),
        }
    }

    fn harness(&mut self) -> Result<&mut Harness, String> {
        self.harness
            .as_mut()
            .ok_or_else(|| "no document loaded: use `load` or `load_html` first".to_string())
    }

    fn load_html(
        &mut self,
        html: &str,
        base_url: Option<String>,
        fixture_dir: Option<&Path>,
    ) -> Value {
        let options = HarnessOptions {
            width: self.viewport.0,
            height: self.viewport.1,
            base_url,
            net_provider: fixture_dir.map(|dir| Arc::new(FileNetProvider::new(dir)) as _),
            ..Default::default()
        };
        self.harness = Some(Harness::from_html_with(html, options));
        json!({ "ok": true })
    }

    fn run(&mut self, command: Command) -> Result<Value, String> {
        match command {
            Command::Load { path } => {
                let path = std::fs::canonicalize(&path).map_err(|err| format!("{path}: {err}"))?;
                let html =
                    std::fs::read_to_string(&path).map_err(|err| format!("read html: {err}"))?;
                let base_url = Some(format!("file://{}", path.display()));
                let fixture_dir = path.parent().map(Path::to_path_buf);
                Ok(self.load_html(&html, base_url, fixture_dir.as_deref()))
            }
            Command::LoadHtml {
                html,
                base_url,
                fixture_dir,
            } => Ok(self.load_html(&html, base_url, fixture_dir.as_deref().map(Path::new))),
            Command::Click { selector, x, y } => {
                let harness = self.harness()?;
                let (x, y) = match (selector, x, y) {
                    (Some(selector), _, _) => {
                        let node = harness
                            .query(&selector)
                            .ok_or_else(|| format!("no element matches `{selector}`"))?;
                        harness.layout_rect_of(node).center()
                    }
                    (None, Some(x), Some(y)) => (x, y),
                    _ => return Err("click requires `selector` or `x` and `y`".to_string()),
                };
                harness.click_at(x, y);
                Ok(json!({ "ok": true, "x": x, "y": y }))
            }
            Command::Move { x, y } => {
                self.harness()?.move_mouse_to(x, y);
                Ok(json!({ "ok": true }))
            }
            Command::Key { key } => {
                self.harness()?.press(parse_key(&key));
                Ok(json!({ "ok": true }))
            }
            Command::Type { text } => {
                self.harness()?.type_text(&text);
                Ok(json!({ "ok": true }))
            }
            Command::Scroll {
                x,
                y,
                delta_x,
                delta_y,
            } => {
                let (vw, vh) = self.viewport;
                let harness = self.harness()?;
                let x = x.unwrap_or(vw as f32 / 2.0);
                let y = y.unwrap_or(vh as f32 / 2.0);
                harness.wheel_at(x, y, delta_x.unwrap_or(0.0), delta_y.unwrap_or(0.0));
                Ok(json!({ "ok": true }))
            }
            Command::DumpDom => {
                let dom = self.harness()?.dom_string();
                Ok(json!({ "ok": true, "dom": dom }))
            }
            Command::DumpPaint => {
                let paint = self.harness()?.paint_string();
                Ok(json!({ "ok": true, "paint": paint }))
            }
            Command::Layout { selector } => {
                let harness = self.harness()?;
                let node = harness
                    .query(&selector)
                    .ok_or_else(|| format!("no element matches `{selector}`"))?;
                let rect = harness.layout_rect_of(node);
                Ok(json!({
                    "ok": true,
                    "node": node.as_u64(),
                    "x": rect.x, "y": rect.y,
                    "width": rect.width, "height": rect.height,
                }))
            }
            Command::Text { selector } => {
                let harness = self.harness()?;
                harness
                    .query(&selector)
                    .ok_or_else(|| format!("no element matches `{selector}`"))?;
                let text = harness.text_content(&selector);
                Ok(json!({ "ok": true, "text": text }))
            }
            Command::Attr { selector, name } => {
                let harness = self.harness()?;
                harness
                    .query(&selector)
                    .ok_or_else(|| format!("no element matches `{selector}`"))?;
                let value = harness.attr(&selector, &name);
                Ok(json!({ "ok": true, "value": value }))
            }
            Command::Value { selector } => {
                let harness = self.harness()?;
                harness
                    .query(&selector)
                    .ok_or_else(|| format!("no element matches `{selector}`"))?;
                let value = harness.input_value(&selector);
                Ok(json!({ "ok": true, "value": value }))
            }
            Command::Query { selector } => {
                let nodes = self.harness()?.query_all(&selector);
                let ids: Vec<u64> = nodes.iter().map(|id| id.as_u64()).collect();
                Ok(json!({ "ok": true, "count": ids.len(), "nodes": ids }))
            }
            Command::Hit { x, y } => {
                let node = self.harness()?.hit(x, y).map(|hit| hit.node_id.as_u64());
                Ok(json!({ "ok": true, "node": node }))
            }
            Command::Screenshot { path } => {
                let shot = self.harness()?.screenshot();
                shot.save_png(&path);
                Ok(json!({
                    "ok": true,
                    "path": path,
                    "width": shot.width,
                    "height": shot.height,
                }))
            }
            Command::SetViewport { width, height } => {
                self.viewport = (width, height);
                if let Ok(harness) = self.harness() {
                    harness.set_viewport_size(width, height);
                }
                Ok(json!({ "ok": true }))
            }
            Command::Tick { seconds } => {
                self.harness()?.tick(seconds);
                Ok(json!({ "ok": true }))
            }
        }
    }
}

fn main() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut driver = Driver::new();

    for line in stdin.lock().lines() {
        let line = line.expect("failed to read stdin");
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Command>(&line) {
            Ok(command) => match driver.run(command) {
                Ok(value) => value,
                Err(error) => json!({ "ok": false, "error": error }),
            },
            Err(error) => json!({ "ok": false, "error": format!("invalid command: {error}") }),
        };
        let mut out = stdout.lock();
        serde_json::to_writer(&mut out, &response).expect("failed to write stdout");
        out.write_all(b"\n").expect("failed to write stdout");
        out.flush().expect("failed to flush stdout");
    }
}
