//! Load an HTML file (by default the Preact TodoMVC example in examples/preact)
//! in a window with JavaScript enabled, using `blitz-script`'s Boa-based script
//! engine.
//!
//! ```sh
//! cargo run --example preact_script [path/to/file.html]
//! ```

use anyrender_vello::VelloWindowRenderer as WindowRenderer;
use blitz_dom::DocumentConfig;
use blitz_script::ScriptDocument;
use blitz_shell::{BlitzApplication, BlitzShellProxy, WindowConfig, create_default_event_loop};

fn main() {
    let raw_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| String::from("examples/preact/index.html"));
    let path = std::path::Path::new(&raw_path)
        .canonicalize()
        .unwrap_or_else(|err| panic!("could not resolve {raw_path}: {err}"));
    let html = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("could not read {}: {err}", path.display()));
    let base_url = url::Url::from_file_path(&path).expect("invalid file path");

    let event_loop = create_default_event_loop();
    let (proxy, receiver) = BlitzShellProxy::new(event_loop.create_proxy());

    let mut doc = ScriptDocument::from_html(
        &html,
        DocumentConfig {
            base_url: Some(base_url.to_string()),
            ..Default::default()
        },
    );
    doc.execute_scripts();

    let window = WindowConfig::new(Box::new(doc) as _, WindowRenderer::new());
    let mut application = BlitzApplication::new(proxy, receiver);
    application.add_window(window);

    event_loop.run_app(application).unwrap()
}
