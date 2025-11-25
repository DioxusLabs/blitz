use std::sync::Arc;

use crate::WindowRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use blitz_net::Provider;
use blitz_shell::{BlitzApplication, BlitzShellEvent, View, WindowConfig};
use blitz_traits::navigation::{NavigationOptions, NavigationProvider};
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
    inner: BlitzApplication<WindowRenderer>,
    handle: tokio::runtime::Handle,
    net_provider: Arc<Provider>,
    raw_url: String,
    keyboard_modifiers: Modifiers,
    navigation_provider: Arc<dyn NavigationProvider>,
    url_history: Vec<String>,
}

impl ReadmeApplication {
    pub fn new(
        proxy: EventLoopProxy<BlitzShellEvent>,
        raw_url: String,
        net_provider: Arc<Provider>,
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

    pub fn add_window(&mut self, window_config: WindowConfig<WindowRenderer>) {
        self.inner.add_window(window_config);
    }

    fn window_mut(&mut self) -> &mut View<WindowRenderer> {
        self.inner.windows.values_mut().next().unwrap()
    }

    fn reload_document(&mut self, retain_scroll_position: bool) {
        let proxy = self.inner.proxy.clone();

        let url = self.raw_url.clone();
        let net_provider = Arc::clone(&self.net_provider);
        self.handle.spawn(async move {
            let url = url;
            let (base_url, contents, is_md, _file_path) = fetch(&url, net_provider).await;
            proxy
                .send_event(BlitzShellEvent::NavigationLoad {
                    url: base_url,
                    contents,
                    is_md,
                    retain_scroll_position,
                })
                .unwrap();
        });
    }

    fn navigate(&mut self, options: NavigationOptions) {
        let proxy = self.inner.proxy.clone();
        self.net_provider.fetch_with_callback(
            options.into_request(),
            Box::new(move |result| {
                let (url, bytes) = result.unwrap();
                let contents = std::str::from_utf8(&bytes).unwrap().to_string();
                proxy
                    .send_event(BlitzShellEvent::NavigationLoad {
                        url,
                        contents,
                        is_md: false,
                        retain_scroll_position: false,
                    })
                    .unwrap();
            }),
        );
    }

    fn load_document(
        &mut self,
        contents: String,
        retain_scroll_position: bool,
        url: String,
        is_md: bool,
    ) {
        let mut html = contents;
        let mut stylesheets = Vec::new();
        if is_md {
            html = markdown_to_html(html);
            stylesheets.push(String::from(GITHUB_MD_STYLES));
            stylesheets.push(String::from(BLITZ_MD_STYLES));
        }

        let doc = HtmlDocument::from_html(
            &html,
            DocumentConfig {
                base_url: Some(url),
                ua_stylesheets: Some(stylesheets),
                net_provider: Some(self.net_provider.clone()),
                navigation_provider: Some(self.navigation_provider.clone()),
                ..Default::default()
            },
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
            BlitzShellEvent::Navigate(options) => {
                let old_url = std::mem::replace(&mut self.raw_url, options.url.to_string());
                self.url_history.push(old_url);
                self.reload_document(false);
                self.navigate(*options);
            }
            BlitzShellEvent::NavigationLoad {
                url,
                contents,
                retain_scroll_position,
                is_md,
            } => {
                self.load_document(contents, retain_scroll_position, url, is_md);
            }
            event => self.inner.user_event(event_loop, event),
        }
    }
}
