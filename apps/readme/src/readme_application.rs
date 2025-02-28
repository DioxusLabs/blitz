use std::sync::Arc;

#[cfg(feature = "gpu")]
use anyrender_vello::VelloWindowRenderer;
#[cfg(feature = "cpu")]
use anyrender_vello_cpu::VelloCpuWindowRenderer as VelloWindowRenderer;

use blitz_dom::net::Resource;
use blitz_html::HtmlDocument;
use blitz_shell::{BlitzApplication, BlitzShellEvent, View, WindowConfig};
use blitz_traits::navigation::NavigationProvider;
use blitz_traits::net::NetProvider;
use tokio::runtime::Handle;
use winit::application::ApplicationHandler;
use winit::event::{Modifiers, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Theme, WindowId};

use crate::fetch;
use crate::markdown::{BLITZ_MD_STYLES, GITHUB_MD_STYLES, markdown_to_html};

pub struct ReadmeEvent;

pub struct ReadmeApplication {
    inner: BlitzApplication<VelloWindowRenderer>,
    handle: tokio::runtime::Handle,
    net_provider: Arc<dyn NetProvider<Resource>>,
    raw_url: String,
    keyboard_modifiers: Modifiers,
    navigation_provider: Arc<dyn NavigationProvider>,
    url_history: Vec<String>,
}

impl ReadmeApplication {
    pub fn new(
        proxy: EventLoopProxy<BlitzShellEvent>,
        raw_url: String,
        net_provider: Arc<dyn NetProvider<Resource>>,
        navigation_provider: Arc<dyn NavigationProvider>,
    ) -> Self {
        let handle = Handle::current();
        Self {
            inner: BlitzApplication::new(proxy.clone()),
            handle,
            raw_url,
            net_provider,
            keyboard_modifiers: Default::default(),
            navigation_provider,
            url_history: Vec::new(),
        }
    }

    pub fn add_window(&mut self, window_config: WindowConfig<VelloWindowRenderer>) {
        self.inner.add_window(window_config);
    }

    fn window_mut(&mut self) -> &mut View<VelloWindowRenderer> {
        self.inner.windows.values_mut().next().unwrap()
    }

    fn reload_document(&mut self, retain_scroll_position: bool) {
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
            self.navigation_provider.clone(),
        );
        self.window_mut()
            .replace_document(Box::new(doc) as _, retain_scroll_position);
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

impl ApplicationHandler<BlitzShellEvent> for ReadmeApplication {
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
                    PhysicalKey::Code(KeyCode::KeyR) => self.reload_document(true),
                    PhysicalKey::Code(KeyCode::KeyT) => self.toggle_theme(),
                    PhysicalKey::Code(KeyCode::KeyB) => {
                        if let Some(url) = self.url_history.pop() {
                            self.raw_url = url;
                            self.reload_document(false);
                        }
                    }
                    _ => {}
                }
            }
        }

        self.inner.window_event(event_loop, window_id, event);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: BlitzShellEvent) {
        match event {
            BlitzShellEvent::Embedder(event) => {
                if let Some(_event) = event.downcast_ref::<ReadmeEvent>() {
                    self.reload_document(true);
                }
            }
            BlitzShellEvent::Navigate(opts) => {
                let old_url = std::mem::replace(&mut self.raw_url, opts.url.into());
                self.url_history.push(old_url);
                self.reload_document(false);
            }
            event => self.inner.user_event(event_loop, event),
        }
    }
}
