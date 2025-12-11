use anyrender::WindowRenderer;
use blitz_shell::{BlitzApplication, View};
use dioxus_core::{provide_context, ScopeId};
use dioxus_history::{History, MemoryHistory};
use std::rc::Rc;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::WindowId;

use crate::windowing::{DioxusWindowHandle, DioxusWindowQueue, DioxusWindowTemplate};
use crate::DioxusNativeWindowRenderer;
use crate::{contexts::DioxusNativeDocument, BlitzShellEvent, DioxusDocument, WindowConfig};

/// Dioxus-native specific event type
pub enum DioxusNativeEvent {
    /// A hotreload event, basically telling us to update our templates.
    #[cfg(all(feature = "hot-reload", debug_assertions))]
    DevserverEvent(dioxus_devtools::DevserverMsg),

    /// Internal signal to create queued windows.
    SpawnQueuedWindows,

    /// Create a new head element from the Link and Title elements
    ///
    /// todo(jon): these should probabkly be synchronous somehow
    CreateHeadElement {
        window: WindowId,
        name: String,
        attributes: Vec<(String, String)>,
        contents: Option<String>,
    },
}

pub struct DioxusNativeApplication {
    pending_window: Option<WindowConfig<DioxusNativeWindowRenderer>>,
    inner: BlitzApplication<DioxusNativeWindowRenderer>,
    proxy: EventLoopProxy<BlitzShellEvent>,
    window_template: Arc<DioxusWindowTemplate>,
    window_queue: Rc<DioxusWindowQueue>,
}

impl DioxusNativeApplication {
    pub(crate) fn new(
        proxy: EventLoopProxy<BlitzShellEvent>,
        config: WindowConfig<DioxusNativeWindowRenderer>,
        window_template: Arc<DioxusWindowTemplate>,
        window_queue: Rc<DioxusWindowQueue>,
    ) -> Self {
        Self {
            pending_window: Some(config),
            inner: BlitzApplication::new(proxy.clone()),
            proxy,
            window_template,
            window_queue,
        }
    }

    pub fn add_window(&mut self, window_config: WindowConfig<DioxusNativeWindowRenderer>) {
        self.inner.add_window(window_config);
    }

    fn spawn_window(
        &mut self,
        config: WindowConfig<DioxusNativeWindowRenderer>,
        event_loop: &ActiveEventLoop,
        auto_resume: bool,
    ) {
        let mut window = View::init(config, event_loop, &self.proxy);
        self.inject_window_contexts(&mut window);
        if auto_resume {
            window.resume();
            if !window.renderer.is_active() {
                return;
            }
        }
        let window_id = window.window_id();
        self.inner.windows.insert(window_id, window);
    }

    fn inject_window_contexts(&self, window: &mut View<DioxusNativeWindowRenderer>) {
        let renderer = window.renderer.clone();
        let window_id = window.window_id();
        let doc = window.downcast_doc_mut::<DioxusDocument>();

        doc.vdom.in_scope(ScopeId::ROOT, || {
            let shared: Rc<dyn dioxus_document::Document> =
                Rc::new(DioxusNativeDocument::new(self.proxy.clone(), window_id));
            provide_context(shared);
        });

        let window_handle = DioxusWindowHandle::new(
            self.proxy.clone(),
            Arc::clone(&self.window_template),
            Rc::clone(&self.window_queue),
        );
        doc.vdom
            .in_scope(ScopeId::ROOT, || provide_context(window_handle.clone()));

        let shell_provider = doc.as_ref().shell_provider.clone();
        doc.vdom
            .in_scope(ScopeId::ROOT, move || provide_context(shell_provider));

        let history_provider: Rc<dyn History> = Rc::new(MemoryHistory::default());
        doc.vdom
            .in_scope(ScopeId::ROOT, move || provide_context(history_provider));

        doc.vdom
            .in_scope(ScopeId::ROOT, move || provide_context(renderer));

        doc.initial_build();
        window.request_redraw();
    }

    fn drain_window_queue(&mut self, event_loop: &ActiveEventLoop) {
        for config in self.window_queue.drain() {
            self.spawn_window(config, event_loop, true);
        }
    }

    fn handle_blitz_shell_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        event: &DioxusNativeEvent,
    ) {
        match event {
            #[cfg(all(feature = "hot-reload", debug_assertions))]
            DioxusNativeEvent::DevserverEvent(event) => match event {
                dioxus_devtools::DevserverMsg::HotReload(hotreload_message) => {
                    for window in self.inner.windows.values_mut() {
                        let doc = window.downcast_doc_mut::<DioxusDocument>();

                        // Apply changes to vdom
                        dioxus_devtools::apply_changes(&doc.vdom, hotreload_message);

                        // Reload changed assets
                        for asset_path in &hotreload_message.assets {
                            if let Some(url) = asset_path.to_str() {
                                doc.inner.borrow_mut().reload_resource_by_href(url);
                            }
                        }

                        window.poll();
                    }
                }
                dioxus_devtools::DevserverMsg::Shutdown => event_loop.exit(),
                dioxus_devtools::DevserverMsg::FullReloadStart => {}
                dioxus_devtools::DevserverMsg::FullReloadFailed => {}
                dioxus_devtools::DevserverMsg::FullReloadCommand => {}
                _ => {}
            },

            DioxusNativeEvent::CreateHeadElement {
                name,
                attributes,
                contents,
                window,
            } => {
                if let Some(window) = self.inner.windows.get_mut(window) {
                    let doc = window.downcast_doc_mut::<DioxusDocument>();
                    doc.create_head_element(name, attributes, contents);
                    window.poll();
                }
            }

            DioxusNativeEvent::SpawnQueuedWindows => {
                self.drain_window_queue(event_loop);
            }

            // Suppress unused variable warning
            #[cfg(not(all(feature = "hot-reload", debug_assertions)))]
            #[allow(unreachable_patterns)]
            _ => {
                let _ = event_loop;
                let _ = event;
            }
        }
    }
}

impl ApplicationHandler<BlitzShellEvent> for DioxusNativeApplication {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(feature = "tracing")]
        tracing::debug!("Injecting document provider into all windows");

        if let Some(config) = self.pending_window.take() {
            self.spawn_window(config, event_loop, false);
        }

        self.drain_window_queue(event_loop);
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
        self.inner.window_event(event_loop, window_id, event);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: BlitzShellEvent) {
        match event {
            BlitzShellEvent::Embedder(event) => {
                if let Some(event) = event.downcast_ref::<DioxusNativeEvent>() {
                    self.handle_blitz_shell_event(event_loop, event);
                }
            }
            event => self.inner.user_event(event_loop, event),
        }
    }
}
