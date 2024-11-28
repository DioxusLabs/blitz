use std::sync::Arc;

use blitz_dom::net::Resource;
use blitz_html::HtmlDocument;
use blitz_shell::{BlitzApplication, BlitzEvent, WindowConfig};
use blitz_traits::net::NetProvider;
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowId;

use crate::fetch;
use crate::markdown::{markdown_to_html, BLITZ_MD_STYLES, GITHUB_MD_STYLES};

pub struct ReadmeEvent;

pub struct ReadmeApplication {
    inner: BlitzApplication<HtmlDocument>,
    handle: tokio::runtime::Handle,
    net_provider: Arc<dyn NetProvider<Data = Resource>>,
    raw_url: String,
}

impl ReadmeApplication {
    pub fn new(
        rt: tokio::runtime::Runtime,
        proxy: EventLoopProxy<BlitzEvent>,
        raw_url: String,
        net_provider: Arc<dyn NetProvider<Data = Resource>>,
    ) -> Self {
        let handle = rt.handle().clone();
        Self {
            inner: BlitzApplication::new(rt, proxy.clone()),
            handle,
            raw_url,
            net_provider,
        }
    }

    pub fn add_window(&mut self, window_config: WindowConfig<HtmlDocument>) {
        self.inner.add_window(window_config);
    }

    fn reload_document(&mut self) {
        let (base_url, contents, is_md, _) = self.handle.block_on(fetch(&self.raw_url));

        let mut html = contents;
        let mut stylesheets = Vec::new();
        if is_md {
            html = markdown_to_html(html);
            stylesheets.push(String::from(GITHUB_MD_STYLES));
            stylesheets.push(String::from(BLITZ_MD_STYLES));
        }

        let doc = HtmlDocument::from_html(
            &html,
            Some(base_url),
            stylesheets,
            self.net_provider.clone(),
            None,
        );
        let window = self.inner.windows.values_mut().next().unwrap();
        window.replace_document(doc);
    }
}

impl ApplicationHandler<BlitzEvent> for ReadmeApplication {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.inner.resumed(event_loop);
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        self.inner.suspended(event_loop);
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        self.inner.new_events(event_loop, cause);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if let WindowEvent::KeyboardInput { event, .. } = &event {
            if event.state.is_pressed()
                && matches!(event.physical_key, PhysicalKey::Code(KeyCode::KeyR))
            {
                self.reload_document();
            }
        }

        self.inner.window_event(event_loop, window_id, event);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: BlitzEvent) {
        match event {
            BlitzEvent::Embedder(event) => {
                if let Some(_event) = event.downcast_ref::<ReadmeEvent>() {
                    self.reload_document();
                }
            }
            event => self.inner.user_event(event_loop, event),
        }
    }
}
