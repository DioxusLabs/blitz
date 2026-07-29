//! Fake-host demo: opens a plain winit window (standing in for a DAW / plugin
//! host) and embeds a Blitz document into it as a baseview child window using
//! `open_parented`. This exercises the same reparenting code path that an audio
//! plugin editor uses, without requiring any plugin host to be installed.
//!
//! Run with: `cargo run --package blitz-shell-baseview --example winit_host`

use anyrender_vello::VelloWindowRenderer;
use blitz_dom::{Document, DocumentConfig};
use blitz_html::HtmlDocument;
use blitz_shell_baseview::dpi::LogicalSize;
use blitz_shell_baseview::{WindowHandle, WindowOpenOptions, WindowScalePolicy};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

static HTML: &str = r#"
<!DOCTYPE html>
<html>
<head>
<style>
    html { background: #0f172a; color: #e2e8f0; font-family: sans-serif; }
    body { margin: 0; padding: 20px; }
    h1 { font-size: 20px; margin: 0 0 8px 0; color: #38bdf8; }
    p { color: #94a3b8; margin: 0 0 16px 0; }
    button {
        background: #1d4ed8; color: white; border: none;
        padding: 8px 16px; border-radius: 6px; cursor: pointer;
    }
    button:hover { background: #2563eb; }
    button:active { background: #1e40af; }
</style>
</head>
<body>
    <h1>Embedded Blitz view</h1>
    <p>This document is a baseview child window parented to the winit "host" window.</p>
    <button>A button</button>
</body>
</html>
"#;

const PLUGIN_WIDTH: f64 = 500.0;
const PLUGIN_HEIGHT: f64 = 300.0;

#[derive(Default)]
struct FakeHost {
    window: Option<Box<dyn Window>>,
    // Dropping the WindowHandle closes the child window, so keep it alive
    blitz_view: Option<WindowHandle>,
}

impl ApplicationHandler for FakeHost {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attrs = WindowAttributes::default()
            .with_title("Fake plugin host (winit)")
            .with_surface_size(LogicalSize::new(PLUGIN_WIDTH, PLUGIN_HEIGHT));
        let window = event_loop.create_window(attrs).unwrap();

        let options = WindowOpenOptions::new()
            .with_size(LogicalSize::new(PLUGIN_WIDTH, PLUGIN_HEIGHT))
            .with_scale_policy(WindowScalePolicy::SystemScaleFactor);

        let parent: &dyn Window = &*window;
        let handle = blitz_shell_baseview::open_parented(&parent, options, || {
            let doc = HtmlDocument::from_html(HTML, DocumentConfig::default());
            let renderer = VelloWindowRenderer::new();
            (Box::new(doc) as Box<dyn Document>, renderer)
        });

        self.window = Some(window);
        self.blitz_view = Some(handle);
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        if matches!(event, WindowEvent::CloseRequested) {
            // Close the child before the parent window is destroyed
            self.blitz_view = None;
            self.window = None;
            event_loop.exit();
        }
    }
}

fn main() {
    let event_loop = EventLoop::builder().build().unwrap();
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop.run_app(FakeHost::default()).unwrap();
}
