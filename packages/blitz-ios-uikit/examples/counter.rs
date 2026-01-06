//! iOS Counter Example - demonstrates blitz-ios-uikit renderer
//!
//! This example creates a simple counter app using the UIKit renderer.
//!
//! # Running
//!
//! iOS Simulator (Apple Silicon Mac):
//! ```sh
//! cargo build --example counter --target aarch64-apple-ios-sim
//! ```
//!
//! iOS Device:
//! ```sh
//! cargo build --example counter --target aarch64-apple-ios
//! ```
//!
//! Mac Catalyst:
//! ```sh
//! cargo build --example counter --target aarch64-apple-ios-macabi
//! ```

#![cfg(target_os = "ios")]

use std::cell::RefCell;
use std::rc::Rc;

use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use blitz_ios_uikit::UIKitRenderer;
use blitz_traits::shell::{ColorScheme, Viewport};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use objc2_ui_kit::UIView;

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
            padding: 40px;
            text-align: center;
        }

        h1 {
            color: #333;
            margin: 0 0 20px 0;
            font-size: 48px;
        }

        .count {
            font-size: 72px;
            font-weight: bold;
            color: #667eea;
            margin: 20px 0;
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
    </style>
</head>
<body>
    <div class="container">
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

    // Parse the HTML document
    println!("1. Parsing HTML document...");
    let doc = HtmlDocument::from_html(HTML, DocumentConfig::default());
    let mut base_doc = doc.into_inner();
    println!("   Document created with root element\n");

    // Set viewport size and compute layout
    println!("2. Computing layout (390x844 - iPhone 14 size)...");
    let viewport = Viewport::new(390, 844, 3.0, ColorScheme::Light);
    base_doc.set_viewport(viewport);
    base_doc.resolve_layout();
    println!("   Layout computed\n");

    // Get main thread marker (required for UIKit)
    println!("3. Getting MainThreadMarker...");
    let mtm = MainThreadMarker::new().expect("Must be called from main thread");
    println!("   MainThreadMarker acquired\n");

    // Create a root UIView
    println!("4. Creating root UIView...");
    let root_frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(390.0, 844.0));
    let root_view = UIView::initWithFrame(mtm.alloc::<UIView>(), root_frame);
    println!("   Root view created: {:?}\n", root_view.frame());

    // Wrap document in Rc<RefCell> for the renderer
    let doc = Rc::new(RefCell::new(base_doc));

    // Create the UIKit renderer
    println!("5. Creating UIKitRenderer...");
    let mut renderer = UIKitRenderer::new(doc.clone(), root_view.clone(), mtm);
    renderer.set_scale(3.0); // iPhone Retina scale
    println!("   Renderer created with scale: {}\n", renderer.scale());

    // Sync the DOM to UIKit views
    println!("6. Syncing DOM to UIKit view hierarchy...");
    renderer.sync();
    println!("   Sync complete!\n");

    // Print view hierarchy info
    println!("7. View hierarchy created:");
    print_view_hierarchy(&root_view, 0);

    println!("\n================================");
    println!("Example complete!");
    println!("\nTo see this running on a real device:");
    println!("  cargo build --example counter --target aarch64-apple-ios");
}

/// Print the UIView hierarchy for debugging
fn print_view_hierarchy(view: &UIView, depth: usize) {
    let indent = "   ".repeat(depth);
    let frame = view.frame();
    let subviews = view.subviews();
    let count = subviews.len();

    println!(
        "{}UIView: origin=({:.0}, {:.0}) size=({:.0}x{:.0}) subviews={}",
        indent, frame.origin.x, frame.origin.y, frame.size.width, frame.size.height, count
    );

    for subview in subviews.iter() {
        print_view_hierarchy(&subview, depth + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_parses() {
        let doc = HtmlDocument::from_html(HTML, DocumentConfig::default());
        let base = doc.into_inner();
        assert!(base.root_element().element_data().is_some());
    }
}
