//! iOS Counter Example - demonstrates blitz-ios-uikit renderer
//!
//! This example creates a simple counter app using the UIKit renderer.
//!
//! # Running
//!
//! Via Dioxus CLI:
//! ```sh
//! dx2 run --ios --example counter
//! ```
//!
//! Direct build for iOS Simulator:
//! ```sh
//! cargo build --example counter --target aarch64-apple-ios-sim
//! ```

#![cfg(target_os = "ios")]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use blitz_dom::{DocumentConfig, local_name};
use blitz_html::HtmlDocument;
use blitz_ios_uikit::UIKitRenderer;
use blitz_net::Provider;
use blitz_traits::shell::{ColorScheme, Viewport};
use objc2::rc::Retained;
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use objc2_ui_kit::UIView;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

// =============================================================================
// HTML Content
// =============================================================================

const HTML: &str = r#"
<!DOCTYPE html>
<html>
<head>
    <style>
        html, body {
            margin: 0;
            padding: 0;
            height: 100%;
            font-family: -apple-system, BlinkMacSystemFont, sans-serif;
        }

        body {
            display: flex;
            flex-direction: column;
            justify-content: center;
            align-items: center;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
        }

        .container {
            background: white;
            border-radius: 20px;
            padding: 30px 50px;
            text-align: center;
            min-width: 280px;
        }

        h1 {
            color: #333;
            margin: 0 0 16px 0;
            font-size: 32px;
        }

        .count {
            font-size: 48px;
            font-weight: bold;
            color: #667eea;
            margin: 16px 0;
        }

        .buttons {
            display: flex;
            gap: 10px;
            justify-content: center;
        }

        button {
            padding: 15px 30px;
            font-size: 24px;
            border: none;
            border-radius: 10px;
        }

        .btn-increment {
            background: #4CAF50;
            color: white;
        }

        .btn-decrement {
            background: #f44336;
            color: white;
        }

        .btn-reset {
            background: #2196F3;
            color: white;
            margin-top: 10px;
        }

        .logo {
            width: 80px;
            height: 80px;
            margin-bottom: 16px;
            border-radius: 16px;
        }
    </style>
</head>
<body>
    <div class="container">
        <img class="logo" src="https://avatars.githubusercontent.com/u/79236386?s=200&v=4" alt="Dioxus Logo" />
        <h1>Counter</h1>
        <div class="count" id="count">0</div>
        <div class="buttons">
            <button class="btn-decrement" id="decrement">-</button>
            <button class="btn-increment" id="increment">+</button>
        </div>
        <button class="btn-reset" id="reset">Reset</button>
    </div>
</body>
</html>
"#;

// =============================================================================
// Application State
// =============================================================================

struct App {
    window: Option<Box<dyn Window>>,
    renderer: Option<UIKitRenderer>,
    doc: Option<Rc<RefCell<blitz_dom::BaseDocument>>>,
    net: Arc<Provider>,
    rt: tokio::runtime::Runtime,
}

impl App {
    fn new() -> Self {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime");

        let net = rt.block_on(async { Arc::new(Provider::new(None)) });

        Self {
            window: None,
            renderer: None,
            doc: None,
            net,
            rt,
        }
    }
}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        println!("[App] can_create_surfaces called");

        // Create window
        let window_attributes =
            winit::window::WindowAttributes::default().with_title("Counter Example");

        let window = event_loop.create_window(window_attributes).unwrap();
        let size = window.surface_size();
        let scale = window.scale_factor() as f32;

        println!(
            "[App] Window created: {}x{} @ {}x scale",
            size.width, size.height, scale
        );

        // Get the UIView from the window handle
        let mtm = MainThreadMarker::new().expect("Must be on main thread");
        let root_view = get_uiview_from_window(&*window, mtm);

        println!("[App] Got UIView from window: {:?}", root_view.frame());

        // Parse HTML and create document with network provider for images
        let viewport = Viewport::new(size.width, size.height, scale, ColorScheme::Light);
        let html_doc = HtmlDocument::from_html(
            HTML,
            DocumentConfig {
                net_provider: Some(Arc::clone(&self.net) as _),
                viewport: Some(viewport.clone()),
                ..Default::default()
            },
        );
        let mut base_doc = html_doc.into_inner();

        // Set viewport
        println!(
            "[App] Setting viewport to {}x{} @ {}x scale",
            size.width, size.height, scale
        );
        base_doc.set_viewport(viewport);

        // Resolve styles and layout, waiting for images to load
        // blitz-net requires a Tokio runtime for async network requests
        println!("[App] Loading assets...");

        // Check if img element exists and has src attribute
        base_doc.visit(|node_id, node| {
            if let Some(element) = node.element_data() {
                if element.name.local.as_ref() == "img" {
                    println!(
                        "[App] Found img element at node {}: src={:?}",
                        node_id,
                        element.attr(local_name!("src"))
                    );
                }
            }
        });

        let net = Arc::clone(&self.net);
        self.rt.block_on(async {
            let mut iterations = 0;
            let mut had_pending = false;
            loop {
                base_doc.resolve(0.0);
                let pending = net.count();
                println!(
                    "[App] Resolve iteration {}, pending requests: {}",
                    iterations, pending
                );

                if pending > 0 {
                    had_pending = true;
                }

                // Only exit after we've seen pending requests AND they're all done
                // AND we've had at least a few iterations to process responses
                if had_pending && net.is_empty() && iterations > 5 {
                    break;
                }

                // Give at least some iterations for requests to be queued
                if iterations > 200 {
                    println!("[App] Max iterations reached, giving up on asset loading");
                    break;
                }
                iterations += 1;
                // Yield to allow network tasks to progress
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }

            // Check image data after loading
            base_doc.visit(|node_id, node| {
                if let Some(element) = node.element_data() {
                    if element.name.local.as_ref() == "img" {
                        println!(
                            "[App] After load - img node {} special_data: {:?}",
                            node_id,
                            std::mem::discriminant(&element.special_data)
                        );
                    }
                }
            });
        });
        println!("[App] Assets loaded");

        // Debug: print root element layout
        let root = base_doc.root_element();
        let layout = root.final_layout;
        println!("[App] Root element layout: {:?}", layout);
        println!("[App] Document layout computed");

        // Wrap document
        let doc = Rc::new(RefCell::new(base_doc));
        self.doc = Some(doc.clone());

        // Create UIKit renderer
        let mut renderer = UIKitRenderer::new(doc, root_view.clone(), mtm);
        renderer.set_scale(scale as f64);

        // Sync DOM to UIKit views
        println!("[App] Syncing DOM to UIKit views...");
        renderer.sync();
        println!("[App] Sync complete!");

        // Debug: print view hierarchy
        print_view_hierarchy(&root_view, 0);

        self.renderer = Some(renderer);
        self.window = Some(window);
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                println!("[App] Close requested");
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                // Re-sync views if needed
                if let Some(renderer) = &mut self.renderer {
                    renderer.sync();
                }
            }
            WindowEvent::SurfaceResized(size) => {
                println!("[App] Resized to {}x{}", size.width, size.height);
                if let (Some(doc), Some(window)) = (&self.doc, &self.window) {
                    let scale = window.scale_factor() as f32;
                    let viewport =
                        Viewport::new(size.width, size.height, scale, ColorScheme::Light);
                    doc.borrow_mut().set_viewport(viewport);
                    doc.borrow_mut().resolve(0.0);
                    if let Some(renderer) = &mut self.renderer {
                        renderer.sync();
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &dyn ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn resumed(&mut self, _event_loop: &dyn ActiveEventLoop) {
        println!("[App] Resumed");
    }
}

/// Extract UIView from a winit Window via raw_window_handle
fn get_uiview_from_window(window: &dyn Window, mtm: MainThreadMarker) -> Retained<UIView> {
    let handle = window.window_handle().expect("Failed to get window handle");
    let raw_handle = handle.as_raw();

    match raw_handle {
        RawWindowHandle::UiKit(uikit_handle) => {
            let ui_view_ptr = uikit_handle.ui_view.as_ptr();
            // SAFETY: The pointer comes from winit's window handle and is valid
            // We're on the main thread (verified by MainThreadMarker)
            unsafe {
                let ui_view: *mut UIView = ui_view_ptr.cast();
                // Retain the view since we're going to use it
                Retained::retain(ui_view).expect("UIView pointer should be valid")
            }
        }
        _ => {
            // Fallback: create a new UIView (won't be attached to window)
            println!("[WARNING] Not a UIKit window handle, creating detached UIView");
            let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(390.0, 844.0));
            UIView::initWithFrame(mtm.alloc::<UIView>(), frame)
        }
    }
}

/// Print the UIView hierarchy for debugging
fn print_view_hierarchy(view: &UIView, depth: usize) {
    let indent = "  ".repeat(depth);
    let frame = view.frame();
    let subviews = view.subviews();

    // Get class name
    let class_name = unsafe {
        use objc2::runtime::AnyObject;
        let obj: &AnyObject = std::mem::transmute(view);
        obj.class().name().to_str().unwrap_or("Unknown")
    };

    println!(
        "{}{}: ({:.0},{:.0}) {:.0}x{:.0} [{}]",
        indent,
        class_name,
        frame.origin.x,
        frame.origin.y,
        frame.size.width,
        frame.size.height,
        subviews.len()
    );

    for subview in subviews.iter() {
        print_view_hierarchy(&subview, depth + 1);
    }
}

// =============================================================================
// Main Entry Point
// =============================================================================

fn main() {
    println!("blitz-ios-uikit Counter Example");
    println!("================================\n");

    let event_loop = EventLoop::new().expect("Failed to create event loop");
    let app = App::new();

    println!("[Main] Starting event loop...");
    event_loop.run_app(app).expect("Event loop failed");
}
