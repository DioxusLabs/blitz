use anyrender_vello::{VelloRendererOptions, VelloWindowRenderer};
use blitz_dom::{qual_name, DocumentConfig};
use blitz_html::HtmlDocument;
use blitz_shell::{create_default_event_loop, BlitzApplication, BlitzShellEvent, WindowConfig};

use crate::{limits, DemoPaintSource, FEATURES, STYLES};

pub fn launch_html() {
    // Create renderer
    let mut renderer = VelloWindowRenderer::with_options(VelloRendererOptions {
        features: Some(FEATURES),
        limits: Some(limits()),
        ..VelloRendererOptions::default()
    });

    // Create custom paint source and register it with the renderer
    let demo_paint_source = Box::new(DemoPaintSource::new());
    let paint_source_id = renderer.register_custom_paint_source(demo_paint_source);

    // Parse the HTML into a Blitz document
    let html = HTML.replace("{{STYLES_PLACEHOLDER}}", STYLES);
    let mut doc = HtmlDocument::from_html(&html, DocumentConfig::default());

    // Set the "src" attribute on the `<canvas>` element to the paint source's id
    // (`<canvas src=".." />` is proprietary blitz extension to HTML)
    let canvas_node_id = doc.query_selector("#demo-canvas").unwrap().unwrap();
    let src_attr = qual_name!("src");
    let src_str = paint_source_id.to_string();
    doc.mutate()
        .set_attribute(canvas_node_id, src_attr, &src_str);

    // Create the Winit application and window
    let event_loop = create_default_event_loop::<BlitzShellEvent>();
    let mut application = BlitzApplication::new(event_loop.create_proxy());
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
            {{STYLES_PLACEHOLDER}}
        </style>
    </head>
    <body>
        <main id="main">
            <div id="overlay">
                <h2>Overlay</h2>
                <p>This overlay demonstrates that the custom WGPU content can be rendered beneath layers of HTML content</p>
            </div>
            <div id="underlay">
                <h2>Underlay</h2>
                <p>This underlay demonstrates that the custom WGPU content can be rendered above layers and blended with the content underneath</p>
            </div>
            <header><h1>Blitz WGPU Demo</h1></header>
            <div id="canvas-container">
                <canvas id="demo-canvas" />
            </div>
        </main>
    </body>
    </html>
"#;
