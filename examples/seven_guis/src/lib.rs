pub mod app;
pub mod tasks;

#[cfg(target_arch = "wasm32")]
mod wasm_entry {
    use std::sync::Arc;

    use blitz_dom::{DocumentConfig, FontContext, decode_font_bytes};
    use blitz_shell::{BlitzShellProxy, WindowConfig};
    use dioxus_core::VirtualDom;
    use dioxus_native::{DioxusDocument, DioxusNativeApplication, DioxusNativeWindowRenderer};
    use parley::fontique::{Blob, Collection, CollectionOptions, GenericFamily, SourceCache};
    use wasm_bindgen::prelude::*;
    use winit::event_loop::{ControlFlow, EventLoop};
    use winit::platform::web::WindowAttributesWeb;
    use winit::window::WindowAttributes;

    /// DejaVu Sans bundled so the document has a real font on wasm32 (browsers
    /// don't expose system fonts to wasm). License: Bitstream Vera / DejaVu (permissive).
    const DEJAVU_SANS: &[u8] = include_bytes!("../assets/DejaVuSans.woff2");

    fn build_font_context() -> FontContext {
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

        let event_loop = EventLoop::new().map_err(|e| JsValue::from_str(&format!("{e}")))?;
        // Wait avoids main-thread saturation but throttles timer-driven renders to
        // ~1Hz on web (winit's web backend uses a long-interval fallback when
        // there's no rAF pending). Poll fixes the timer but lags the address bar.
        event_loop.set_control_flow(ControlFlow::Wait);
        let winit_proxy = event_loop.create_proxy();
        let (proxy, event_queue) = BlitzShellProxy::new(winit_proxy);

        let vdom = VirtualDom::new(super::app::app);
        let doc = DioxusDocument::new(
            vdom,
            DocumentConfig {
                font_ctx: Some(build_font_context()),
                ..Default::default()
            },
        );

        // DioxusNativeWindowRenderer wraps VelloHybridWindowRenderer when the
        // `vello-hybrid` feature is active (dioxus_renderer.rs:28-29).
        let renderer = DioxusNativeWindowRenderer::new();

        // Intentionally no `.with_surface_size(...)` on wasm: letting winit-web set the
        // canvas size writes fixed inline CSS (canvas.style.width/height) that overrides
        // host stylesheet rules and suppresses ResizeObserver. Host CSS sizes the canvas.
        let attrs = WindowAttributes::default()
            .with_platform_attributes(Box::new(WindowAttributesWeb::default().with_append(true)));

        let config = WindowConfig::with_attributes(Box::new(doc) as _, renderer, attrs);
        let application = DioxusNativeApplication::new(proxy, event_queue, config);

        event_loop
            .run_app(application)
            .map_err(|e| JsValue::from_str(&format!("{e}")))?;
        Ok(())
    }
}
