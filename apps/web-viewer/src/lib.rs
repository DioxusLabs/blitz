//! Renders one web page into a canvas, for embedding in a page that provides
//! the browser UI (URL bar, back/forward) in HTML.
//!
//! - `?url=<page>` is the page to load first.
//! - `?proxy=<prefix>` (resolved against this page's URL) routes every http(s)
//!   fetch through a proxy as `<prefix><url-encoded target>`, since browsers
//!   only let a page fetch cross-origin resources that send CORS headers.
//!
//! It talks to `window.parent` with `postMessage`:
//! - sends `{type: "blitz:ready"}` once running,
//! - sends `{type: "blitz:navigated", url, title}` after each page load
//!   (including link clicks) and `{type: "blitz:error", url, message}` when a
//!   page fails to load,
//! - accepts `{type: "blitz:navigate", url}` to load another page.
//!
//! Build with `trunk build --release --public-url ./` in this directory.

#![cfg(target_arch = "wasm32")]

use std::sync::{Arc, LazyLock, OnceLock};

use anyrender_vello_hybrid::VelloHybridWebGlWindowRenderer as WindowRenderer;
use blitz_dom::{DocumentConfig, FontContext, decode_font_bytes};
use blitz_html::HtmlDocument;
use blitz_net::Provider;
use blitz_shell::{BlitzApplication, BlitzShellEvent, BlitzShellProxy, View, WindowConfig};
use blitz_traits::navigation::{NavigationOptions, NavigationProvider};
use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request};
use parley::fontique::{Blob, Collection, CollectionOptions, GenericFamily, SourceCache};
use url::Url;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::web::WindowAttributesWeb;
use winit::window::{WindowAttributes, WindowId};

/// DejaVu Sans, bundled because browsers don't expose system fonts to wasm.
/// License: Bitstream Vera / DejaVu (permissive).
const DEJAVU_SANS: &[u8] = include_bytes!("../assets/DejaVuSans.woff2");

static PROXY_PREFIX: OnceLock<String> = OnceLock::new();

/// The URL to fetch `url` from: through the proxy if one is set.
fn fetch_url(url: &Url) -> Url {
    let Some(prefix) = PROXY_PREFIX.get() else {
        return url.clone();
    };
    if !matches!(url.scheme(), "http" | "https") {
        return url.clone();
    }
    let encoded: String = url::form_urlencoded::byte_serialize(url.as_str().as_bytes()).collect();
    Url::parse(&format!("{prefix}{encoded}")).unwrap_or_else(|_| url.clone())
}

/// Fetches a document's subresources through the proxy, reporting responses
/// under their original URL so relative URLs resolve against the real page.
struct ProxyNetProvider(Arc<Provider>);

impl NetProvider for ProxyNetProvider {
    fn fetch(&self, doc_id: usize, mut request: Request, handler: Box<dyn NetHandler>) {
        let original_url = request.url.to_string();
        request.url = fetch_url(&request.url);
        let handler = Box::new(OriginalUrlHandler {
            original_url,
            inner: handler,
        });
        self.0.fetch(doc_id, request, handler)
    }
}

struct OriginalUrlHandler {
    original_url: String,
    inner: Box<dyn NetHandler>,
}

impl NetHandler for OriginalUrlHandler {
    fn bytes(self: Box<Self>, _resolved_url: String, bytes: Bytes) {
        self.inner.bytes(self.original_url, bytes)
    }

    fn error(self: Box<Self>, _url: String, error: String) {
        self.inner.error(self.original_url, error)
    }
}

struct ViewerNavigationProvider(BlitzShellProxy);

impl NavigationProvider for ViewerNavigationProvider {
    fn navigate_to(&self, opts: NavigationOptions) {
        self.0.send_event(BlitzShellEvent::Navigate(Box::new(opts)));
    }
}

enum ViewerEvent {
    /// The parent page asked to load a URL
    Navigate(String),
    /// A page failed to load
    LoadError { url: String, message: String },
}

fn font_context() -> FontContext {
    static FONT: LazyLock<Blob<u8>> =
        LazyLock::new(|| Blob::new(Arc::new(decode_font_bytes(DEJAVU_SANS).into_owned()) as _));
    let mut ctx = FontContext {
        source_cache: SourceCache::new_shared(),
        collection: Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        }),
    };
    let registered = ctx.collection.register_fonts(FONT.clone(), None);
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

fn post_to_parent(fields: &[(&str, &str)]) {
    let Some(parent) = web_sys::window().and_then(|w| w.parent().ok().flatten()) else {
        return;
    };
    let message = js_sys::Object::new();
    for (key, value) in fields {
        let _ = js_sys::Reflect::set(&message, &(*key).into(), &(*value).into());
    }
    let _ = parent.post_message(&message, "*");
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

struct ViewerApplication {
    inner: BlitzApplication<WindowRenderer>,
    net_provider: Arc<Provider>,
    doc_net_provider: Arc<dyn NetProvider>,
    navigation_provider: Arc<dyn NavigationProvider>,
}

impl ViewerApplication {
    fn window_mut(&mut self) -> &mut View<WindowRenderer> {
        self.inner.windows.values_mut().next().unwrap()
    }

    fn load(&mut self, mut request: Request) {
        let url = request.url.to_string();
        request.url = fetch_url(&request.url);
        let proxy = self.inner.proxy.clone();
        self.net_provider.fetch_with_callback(
            request,
            Box::new(move |result| match result {
                Ok((_, bytes)) => proxy.send_event(BlitzShellEvent::NavigationLoad {
                    url,
                    contents: String::from_utf8_lossy(&bytes).into_owned(),
                    is_md: false,
                    retain_scroll_position: false,
                }),
                Err(err) => proxy.send_event(BlitzShellEvent::Embedder(Arc::new(
                    ViewerEvent::LoadError {
                        url,
                        message: format!("{err:?}"),
                    },
                ))),
            }),
        );
    }

    fn load_url(&mut self, raw_url: &str) {
        match Url::parse(raw_url) {
            Ok(url) => self.load(Request::get(url)),
            Err(err) => self.show_error(raw_url, &err.to_string()),
        }
    }

    fn show_document(&mut self, html: &str, url: Option<String>) {
        let doc = HtmlDocument::from_html(
            html,
            DocumentConfig {
                base_url: url,
                net_provider: Some(self.doc_net_provider.clone()),
                navigation_provider: Some(self.navigation_provider.clone()),
                font_ctx: Some(font_context()),
                ..Default::default()
            },
        );
        self.window_mut().replace_document(Box::new(doc), false);
    }

    fn show_error(&mut self, url: &str, message: &str) {
        let html = format!(
            "<h1>Failed to load page</h1><p>{}</p><pre style=\"white-space: pre-wrap\">{}</pre>",
            escape_html(url),
            escape_html(message),
        );
        self.show_document(&html, None);
        post_to_parent(&[("type", "blitz:error"), ("url", url), ("message", message)]);
    }
}

impl ApplicationHandler for ViewerApplication {
    fn resumed(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.resumed(event_loop);
    }

    fn suspended(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.suspended(event_loop);
    }

    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.can_create_surfaces(event_loop);
    }

    fn destroy_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        self.inner.destroy_surfaces(event_loop);
    }

    fn new_events(&mut self, event_loop: &dyn ActiveEventLoop, cause: StartCause) {
        self.inner.new_events(event_loop, cause);
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        self.inner.window_event(event_loop, window_id, event);
    }

    fn proxy_wake_up(&mut self, event_loop: &dyn ActiveEventLoop) {
        while let Ok(event) = self.inner.event_queue.try_recv() {
            match event {
                BlitzShellEvent::Embedder(event) => match event.downcast_ref::<ViewerEvent>() {
                    Some(ViewerEvent::Navigate(url)) => self.load_url(url),
                    Some(ViewerEvent::LoadError { url, message }) => self.show_error(url, message),
                    None => {}
                },
                BlitzShellEvent::Navigate(options) => self.load(options.into_request()),
                BlitzShellEvent::NavigationLoad { url, contents, .. } => {
                    self.show_document(&contents, Some(url.clone()));
                    let title = self
                        .window_mut()
                        .doc
                        .inner()
                        .find_title_node()
                        .map(|node| node.text_content().trim().to_string())
                        .unwrap_or_default();
                    post_to_parent(&[
                        ("type", "blitz:navigated"),
                        ("url", &url),
                        ("title", &title),
                    ]);
                }
                event => self.inner.handle_blitz_shell_event(event_loop, event),
            }
        }
    }
}

/// Forwards `{type: "blitz:navigate", url}` messages from the parent page.
fn listen_for_parent_messages(window: &web_sys::Window, proxy: BlitzShellProxy) {
    let parent = window.parent().ok().flatten();
    let on_message =
        Closure::<dyn Fn(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            let from_parent = match (&parent, event.source()) {
                (Some(parent), Some(source)) => js_sys::Object::is(parent, &source),
                _ => false,
            };
            if !from_parent {
                return;
            }
            let data = event.data();
            let field = |name: &str| js_sys::Reflect::get(&data, &name.into()).ok()?.as_string();
            if field("type").as_deref() == Some("blitz:navigate")
                && let Some(url) = field("url")
            {
                proxy.send_event(BlitzShellEvent::Embedder(Arc::new(ViewerEvent::Navigate(
                    url,
                ))));
            }
        });
    let _ = window.add_event_listener_with_callback("message", on_message.as_ref().unchecked_ref());
    on_message.forget();
}

#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();

    let window = web_sys::window().expect("no global window");
    let page = Url::parse(&window.location().href()?).map_err(|e| e.to_string())?;
    let param = |name: &str| {
        page.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };
    if let Some(prefix) = param("proxy").and_then(|p| page.join(&p).ok()) {
        let _ = PROXY_PREFIX.set(prefix.to_string());
    }
    let start_url = param("url");

    let canvas = window
        .document()
        .and_then(|d| d.get_element_by_id("blitz-target"))
        .ok_or("no #blitz-target canvas")?
        .dyn_into::<web_sys::HtmlCanvasElement>()?;
    canvas.set_tab_index(0);

    let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    let (proxy, event_queue) = BlitzShellProxy::new(event_loop.create_proxy());
    listen_for_parent_messages(&window, proxy.clone());

    let net_provider = Arc::new(Provider::new(Some(Arc::new(proxy.clone()) as _)));
    let mut app = ViewerApplication {
        inner: BlitzApplication::new(proxy.clone(), event_queue),
        doc_net_provider: Arc::new(ProxyNetProvider(net_provider.clone())),
        net_provider,
        navigation_provider: Arc::new(ViewerNavigationProvider(proxy)),
    };

    let doc = HtmlDocument::from_html(
        "",
        DocumentConfig {
            font_ctx: Some(font_context()),
            ..Default::default()
        },
    );
    // Host CSS sizes the canvas, so no `with_surface_size` (on the web it
    // writes fixed inline CSS that stops the canvas tracking the page size).
    let attrs = WindowAttributes::default().with_platform_attributes(Box::new(
        WindowAttributesWeb::default().with_canvas(Some(canvas)),
    ));
    app.inner.add_window(WindowConfig::with_attributes(
        Box::new(doc),
        WindowRenderer::new(),
        attrs,
    ));
    if let Some(url) = start_url {
        app.load_url(&url);
    }

    post_to_parent(&[("type", "blitz:ready")]);
    event_loop.run_app(app).map_err(|e| e.to_string())?;
    Ok(())
}
