use std::sync::Arc;

use anyrender_vello::VelloWindowRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_shell::{BlitzApplication, BlitzShellEvent, WindowConfig, create_default_event_loop};

pub fn main() {
    // Create renderer

    // Parse the HTML into a Blitz document
    let mut doc = HtmlDocument::from_html(
        HTML,
        DocumentConfig {
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );

    let node_id = doc.query_selector("#content_area").unwrap().unwrap();
    doc.mutate().set_inner_html(node_id, INNER_HTML);
    doc.resolve(0.0);

    // Create the Winit application and window
    let event_loop = create_default_event_loop::<BlitzShellEvent>();
    let mut application = BlitzApplication::new(event_loop.create_proxy());
    let renderer = VelloWindowRenderer::new();
    let window = WindowConfig::new(Box::new(doc), renderer);
    application.add_window(window);

    // Run event loop
    event_loop.run_app(&mut application).unwrap()
}

static HTML: &str = r#"
    <!DOCTYPE html>
    <html>
    <head>
        <style type="text/css">
            #content_area {
                padding: 12px;
                border: 1px solid black;
            }
        </style>
    </head>
    <body>
        <main id="main">
            <h1>Inner HTML Demo</h1>
            <p>Text set with innerHTML should appear below:</p>
            <div id="content_area"></div>
        </main>
    </body>
    </html>
"#;

static INNER_HTML: &str = r#"
    INNER <b>HTML</b> <i>TEXT</i> HERE
"#;
