//! Minimal WASM proof: drives `BlitzApplication` on `wasm32-unknown-unknown` and
//! renders a static HTML payload to a canvas.
//!
//! Build with: `trunk serve` from this directory.

use std::sync::Arc;

use anyrender_vello_hybrid::VelloHybridWindowRenderer;
use blitz_dom::{DocumentConfig, FontContext, decode_font_bytes};
use blitz_html::HtmlDocument;
use blitz_shell::{BlitzApplication, BlitzShellProxy, WindowConfig};
use parley::fontique::{Blob, Collection, CollectionOptions, GenericFamily, SourceCache};
use tracing::info;
use wasm_bindgen::prelude::*;
use winit::dpi::LogicalSize;
use winit::event_loop::EventLoop;
use winit::platform::web::WindowAttributesWeb;
use winit::window::WindowAttributes;

/// DejaVu Sans, bundled so the document has a real font on wasm32 (browsers don't
/// expose system fonts to wasm). License: Bitstream Vera / DejaVu (permissive).
const DEJAVU_SANS: &[u8] = include_bytes!("../assets/DejaVuSans.woff2");

const HTML: &str = r#"<!doctype html>
<html>
<head><title>blitz-shell on WASM</title></head>
<body>
  <main>
    <h1>blitz-shell, running in your browser</h1>
    <p class="lede">
      Laid out by <strong>blitz-dom</strong>, painted by <strong>blitz-paint</strong>,
      rendered through <strong>anyrender</strong>'s vello hybrid backend &mdash;
      driven by the same <code>BlitzApplication</code> that runs natively.
    </p>
    <ul>
      <li>winit creates the canvas and dispatches events</li>
      <li><code>WindowRenderer::resume</code> spawns wgpu init onto the JS microtask queue</li>
      <li>A <code>BlitzShellEvent::ResumeReady</code> finalizes the first frame</li>
    </ul>
  </main>
</body>
<style>
  html, body {
    margin: 0;
    background: #0f1226;
    color: #e6e7ee;
    font-family: sans-serif;
    line-height: 1.5;
  }
  main {
    max-width: 640px;
    margin: 56px auto;
    padding: 32px 36px;
    background: #1a1d3a;
    border: 1px solid #2a2f5a;
    border-radius: 12px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.45);
  }
  h1 {
    margin: 0 0 18px 0;
    color: #ffd166;
    font-size: 26px;
  }
  .lede { font-size: 16px; color: #cfd1e0; }
  ul { padding-left: 20px; color: #cfd1e0; }
  li { margin: 6px 0; }
  code, strong { color: #ff7b9c; }
  code { font-family: monospace; font-size: 0.95em; }
  strong { font-weight: 600; cursor: pointer; }
  strong:hover { text-decoration: underline; }
</style>
</html>"#;

fn build_font_context() -> FontContext {
    // Browsers don't expose system fonts to wasm, so register a bundled font and
    // alias it to every generic family the stylesheet might ask for.
    let mut ctx = FontContext {
        source_cache: SourceCache::new_shared(),
        collection: Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        }),
    };
    let font_bytes = decode_font_bytes(DEJAVU_SANS).into_owned();
    let registered = ctx
        .collection
        .register_fonts(Blob::new(Arc::new(font_bytes) as _), None);
    let family_ids: Vec<_> = registered.iter().map(|(id, _)| *id).collect();
    for generic in [
        GenericFamily::SansSerif,
        GenericFamily::Serif,
        GenericFamily::Monospace,
        GenericFamily::SystemUi,
    ] {
        ctx.collection
            .append_generic_families(generic, family_ids.iter().copied());
    }
    ctx
}

#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    tracing_wasm::set_as_global_default();

    info!("Starting app...");
    let window = web_sys::window().expect("global window does not exists");
    let document = window.document().expect("expecting a document on window");
    let canvas = document.get_element_by_id("blitz-target").unwrap();
    let canvas = canvas.dyn_into::<web_sys::HtmlCanvasElement>().unwrap();

    let width = canvas.offset_width() as u32;
    let height = canvas.offset_height() as u32;

    // Make sure the canvas can be given focus.
    // https://developer.mozilla.org/en-US/docs/Web/HTML/Global_attributes/tabindex
    canvas.set_tab_index(0);

    // Don't outline the canvas when it has focus:
    canvas.style().set_property("outline", "none")?;

    let event_loop = EventLoop::new().map_err(|e| JsValue::from_str(&format!("{e}")))?;
    let (proxy, rx) = BlitzShellProxy::new(event_loop.create_proxy());

    let renderer = VelloHybridWindowRenderer::new();
    let doc = HtmlDocument::from_html(
        HTML,
        DocumentConfig {
            font_ctx: Some(build_font_context()),
            ..Default::default()
        },
    );

    // winit-web requires an attached canvas with a definite size before surface
    // creation, so request the size up front and let winit auto-append the canvas.
    let attrs = WindowAttributes::default()
        .with_surface_size(LogicalSize::new(width, height))
        .with_platform_attributes(Box::new(
            WindowAttributesWeb::default().with_canvas(Some(canvas)),
        ));
    let window_config = WindowConfig::with_attributes(Box::new(doc), renderer, attrs);

    let mut app = BlitzApplication::<VelloHybridWindowRenderer>::new(proxy, rx);
    app.add_window(window_config);

    event_loop
        .run_app(app)
        .map_err(|e| JsValue::from_str(&format!("{e}")))?;
    Ok(())
}
