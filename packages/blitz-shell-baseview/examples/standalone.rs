//! Standalone (no host) demo: opens a Blitz document in a top-level baseview
//! window using `open_blocking`.
//!
//! Run with: `cargo run --package blitz-shell-baseview --example standalone`

use anyrender_vello::VelloWindowRenderer;
use blitz_dom::{Document, DocumentConfig};
use blitz_html::HtmlDocument;
use blitz_shell_baseview::dpi::LogicalSize;
use blitz_shell_baseview::{WindowOpenOptions, WindowScalePolicy};

static HTML: &str = r#"
<!DOCTYPE html>
<html>
<head>
<style>
    html { background: #18181b; color: #e4e4e7; font-family: sans-serif; }
    body { margin: 0; padding: 24px; }
    h1 { font-size: 24px; margin: 0 0 8px 0; }
    p { color: #a1a1aa; margin: 0 0 24px 0; }
    .knobs { display: flex; gap: 16px; }
    .knob {
        width: 90px; height: 90px;
        border-radius: 50%;
        background: radial-gradient(circle at 30% 30%, #3f3f46, #27272a);
        border: 2px solid #52525b;
        display: flex; align-items: center; justify-content: center;
        color: #fafafa; font-size: 13px;
        cursor: pointer;
    }
    .knob:hover { border-color: #a78bfa; color: #a78bfa; }
    .knob:active { background: radial-gradient(circle at 30% 30%, #4c1d95, #27272a); }
    form { margin-top: 24px; display: flex; gap: 8px; align-items: center; }
    input[type=text] {
        background: #27272a; border: 1px solid #52525b; color: #e4e4e7;
        padding: 6px 10px; border-radius: 6px;
    }
    input[type=range] { width: 200px; }
</style>
</head>
<body>
    <h1>Blitz &times; baseview</h1>
    <p>An HTML/CSS plugin GUI rendered by Blitz inside a baseview window.</p>
    <div class="knobs">
        <div class="knob">CUTOFF</div>
        <div class="knob">RES</div>
        <div class="knob">DRIVE</div>
    </div>
    <form>
        <input type="range" min="0" max="100" value="50" />
        <input type="text" value="Preset 1" />
    </form>
</body>
</html>
"#;

fn main() {
    let options = WindowOpenOptions::new()
        .with_title("Blitz baseview demo")
        .with_size(LogicalSize::new(640.0, 400.0))
        .with_scale_policy(WindowScalePolicy::SystemScaleFactor);

    blitz_shell_baseview::open_blocking(options, || {
        let doc = HtmlDocument::from_html(HTML, DocumentConfig::default());
        let renderer = VelloWindowRenderer::new();
        (Box::new(doc) as Box<dyn Document>, renderer)
    });
}
