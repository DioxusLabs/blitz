//! iOS Counter Example - demonstrates blitz-ios-uikit renderer
//!
//! This example creates a simple counter app using the UIKit renderer with
//! proper event loop integration for retained UI.
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

use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use blitz_ios_uikit::{UIKitApplication, UIKitProxy, ViewConfig};
use blitz_net::Provider;
use blitz_traits::shell::{ColorScheme, Viewport};
use winit::event_loop::EventLoop;

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
// Main Entry Point
// =============================================================================

fn main() {
    println!("blitz-ios-uikit Counter Example");
    println!("================================\n");

    // Create a Tokio runtime for network operations
    // Note: In a real app, you might want to manage this differently
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create Tokio runtime");

    // Create event loop
    let event_loop = EventLoop::new().expect("Failed to create event loop");

    rt.block_on(async move {
        // Create proxy for event loop communication (used by network provider)
        let (proxy, receiver) = UIKitProxy::new(event_loop.create_proxy());

        // Create network provider with the proxy as the waker
        // This allows network requests to wake the event loop when they complete
        let net = Arc::new(Provider::new(Some(Arc::new(proxy.clone()))));

        // Parse HTML and create document
        // Use a placeholder viewport - it will be updated when the window is created
        let viewport = Viewport::new(390, 844, 3.0, ColorScheme::Light);
        let html_doc = HtmlDocument::from_html(
            HTML,
            DocumentConfig {
                net_provider: Some(Arc::clone(&net) as _),
                viewport: Some(viewport),
                ..Default::default()
            },
        );
        let base_doc = html_doc.into_inner();

        // Pre-load assets (images, fonts, etc.) using the Tokio runtime
        // This is optional but provides a smoother initial render
        let doc = Rc::new(RefCell::new(base_doc));

        let mut iterations = 0;
        loop {
            doc.borrow_mut().resolve(0.0);
            let pending = net.count();

            if pending == 0 && iterations > 2 {
                break;
            }
            if iterations > 50 {
                println!("[App] Max iterations reached for asset loading");
                break;
            }
            iterations += 1;
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        println!("[App] Initial assets loaded");

        // Create the application with the proxy and receiver
        let mut app = UIKitApplication::new(proxy, receiver);

        // Add the view configuration
        app.add_view(ViewConfig::new(doc));

        println!("[Main] Starting event loop...");
        event_loop.run_app(app).expect("Event loop failed");
    });
}
