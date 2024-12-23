use std::sync::Arc;

use blitz_dom::net::Resource;
use blitz_html::HtmlDocument;
use blitz_renderer_vello::BlitzVelloRenderer;
use blitz_shell::{BlitzApplication, BlitzEvent, View, WindowConfig};
use blitz_traits::net::NetProvider;
use tokio::runtime::Handle;
use winit::application::ApplicationHandler;
use winit::event::{Modifiers, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Theme, WindowId};

use crate::fetch;
use crate::markdown::{markdown_to_html, BLITZ_MD_STYLES, GITHUB_MD_STYLES};

pub struct ReadmeEvent;

pub struct ReadmeApplication {
    inner: BlitzApplication<HtmlDocument, BlitzVelloRenderer>,
    handle: tokio::runtime::Handle,
    net_provider: Arc<dyn NetProvider<Data = Resource>>,
    raw_url: String,
    keyboard_modifiers: Modifiers,
}

impl ReadmeApplication {
    pub fn new(
        proxy: EventLoopProxy<BlitzEvent>,
        raw_url: String,
        net_provider: Arc<dyn NetProvider<Data = Resource>>,
    ) -> Self {
        let handle = Handle::current();
        Self {
            inner: BlitzApplication::new(proxy.clone()),
            handle,
            raw_url,
            net_provider,
            keyboard_modifiers: Default::default(),
        }
    }

    pub fn add_window(&mut self, window_config: WindowConfig<HtmlDocument, BlitzVelloRenderer>) {
        self.inner.add_window(window_config);
    }

    fn window_mut(&mut self) -> &mut View<HtmlDocument, BlitzVelloRenderer> {
        self.inner.windows.values_mut().next().unwrap()
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
        self.window_mut().replace_document(doc);
    }

    fn toggle_theme(&mut self) {
        let window = self.window_mut();
        let new_theme = match window.current_theme() {
            Theme::Light => Theme::Dark,
            Theme::Dark => Theme::Light,
        };
        window.set_theme_override(Some(new_theme));
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
        if let WindowEvent::ModifiersChanged(new_state) = &event {
            self.keyboard_modifiers = *new_state;
        }

        if let WindowEvent::KeyboardInput { event, .. } = &event {
            let mods = self.keyboard_modifiers.state();
            if !event.state.is_pressed() && (mods.control_key() || mods.super_key()) {
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::KeyR) => self.reload_document(),
                    PhysicalKey::Code(KeyCode::KeyT) => self.toggle_theme(),
                    _ => {}
                }
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
