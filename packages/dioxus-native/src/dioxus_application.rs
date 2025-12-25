use blitz_shell::WindowConfig;
use blitz_shell::{BlitzApplication, View};
use blitz_traits::{navigation::NavigationProvider, net::NetProvider};
use dioxus_core::{provide_context, ScopeId};
use dioxus_history::{History, MemoryHistory};
use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::oneshot;
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::Window;
use winit::window::WindowId;

use blitz_dom::DocumentConfig;
use blitz_dom::HtmlParserProvider;

use crate::DioxusNativeWindowRenderer;
use crate::{contexts::DioxusNativeDocument, BlitzShellEvent, DioxusDocument};

#[repr(transparent)]
#[doc(hidden)]
pub struct OpaquePtr<T: ?Sized>(*mut T);

impl<T: ?Sized> Copy for OpaquePtr<T> {}

impl<T: ?Sized> Clone for OpaquePtr<T> {
    fn clone(&self) -> Self {
        *self
    }
}

// Safety: this is intentionally used to smuggle single-threaded payloads through an embedder
// event channel that requires `Send + Sync`. Correctness relies on the caller ensuring that the
// payload is only ever created + consumed within the same thread/affinity assumptions as before.
unsafe impl<T: ?Sized> Send for OpaquePtr<T> {}
unsafe impl<T: ?Sized> Sync for OpaquePtr<T> {}

impl<T: ?Sized> OpaquePtr<T> {
    pub fn from_box(value: Box<T>) -> Self {
        Self(Box::into_raw(value))
    }
}

#[doc(hidden)]
pub struct UnsafeBox<T: ?Sized>(Box<T>);

// Safety: this wrapper exists solely to satisfy the `Send + Sync` bound imposed by the embedder
// event channel. The payloads are only ever created and consumed on the event loop thread.
unsafe impl<T: ?Sized> Send for UnsafeBox<T> {}
unsafe impl<T: ?Sized> Sync for UnsafeBox<T> {}

impl<T: ?Sized> UnsafeBox<T> {
    pub fn new(value: Box<T>) -> Self {
        Self(value)
    }

    pub fn into_inner(self) -> Box<T> {
        self.0
    }
}

type ContextProvider = Box<dyn Fn() -> Box<dyn Any> + Send + Sync>;
type ContextProviders = Arc<Vec<ContextProvider>>;
type WindowCreated = (WindowId, Arc<Window>);
type CreateWindowReply = UnsafeBox<oneshot::Sender<WindowCreated>>;
type CreateWindowReplyOpt = Option<CreateWindowReply>;

/// Dioxus-native specific event type
pub enum DioxusNativeEvent {
    /// A hotreload event, basically telling us to update our templates.
    #[cfg(all(feature = "hot-reload", debug_assertions))]
    DevserverEvent(dioxus_devtools::DevserverMsg),

    /// Create a new head element from the Link and Title elements
    ///
    /// todo(jon): these should probably be synchronous somehow
    CreateHeadElement {
        window: WindowId,
        name: String,
        attributes: Vec<(String, String)>,
        contents: Option<String>,
    },

    /// Spawn a pre-constructed window.
    ///
    /// # Safety
    /// The pointers must come from `Box::into_raw` on the same process, and must be sent exactly
    /// once; they will be reclaimed by the event loop thread via `Box::from_raw`.
    CreateDocumentWindow {
        vdom: UnsafeBox<dioxus_core::VirtualDom>,
        attributes: winit::window::WindowAttributes,
        reply: CreateWindowReplyOpt,
    },

    GetWindow {
        window_id: WindowId,
        reply: UnsafeBox<oneshot::Sender<Option<Arc<Window>>>>,
    },
}

pub struct DioxusNativeApplication {
    inner: BlitzApplication<DioxusNativeWindowRenderer>,
    proxy: EventLoopProxy<BlitzShellEvent>,

    renderer_factory: Arc<dyn Fn() -> DioxusNativeWindowRenderer + Send + Sync>,

    contexts: ContextProviders,
    net_provider: Arc<dyn NetProvider>,
    #[cfg(feature = "net")]
    inner_net_provider: Option<Arc<blitz_net::Provider>>,
    html_parser_provider: Option<Arc<dyn HtmlParserProvider>>,
    navigation_provider: Option<Arc<dyn NavigationProvider>>,
}

#[derive(Clone)]
pub struct DioxusNativeProvider {
    proxy: EventLoopProxy<BlitzShellEvent>,
}

impl DioxusNativeProvider {
    pub(crate) fn new(proxy: EventLoopProxy<BlitzShellEvent>) -> Self {
        Self { proxy }
    }

    pub fn create_document_window(
        &self,
        vdom: dioxus_core::VirtualDom,
        attributes: winit::window::WindowAttributes,
    ) -> oneshot::Receiver<(WindowId, Arc<Window>)> {
        let (sender, receiver) = oneshot::channel();
        let vdom = UnsafeBox::new(Box::new(vdom));
        let reply = Some(UnsafeBox::new(Box::new(sender)));
        let _ = self.proxy.send_event(BlitzShellEvent::embedder_event(
            DioxusNativeEvent::CreateDocumentWindow {
                vdom,
                attributes,
                reply,
            },
        ));
        receiver
    }

    pub fn get_window(&self, window_id: WindowId) -> oneshot::Receiver<Option<Arc<Window>>> {
        let (sender, receiver) = oneshot::channel();
        let reply = UnsafeBox::new(Box::new(sender));
        let _ = self.proxy.send_event(BlitzShellEvent::embedder_event(
            DioxusNativeEvent::GetWindow { window_id, reply },
        ));
        receiver
    }
}

impl DioxusNativeApplication {
    pub(crate) fn new(
        proxy: EventLoopProxy<BlitzShellEvent>,
        renderer_factory: Arc<dyn Fn() -> DioxusNativeWindowRenderer + Send + Sync>,
        contexts: ContextProviders,
        net_provider: Arc<dyn NetProvider>,
        #[cfg(feature = "net")] inner_net_provider: Option<Arc<blitz_net::Provider>>,
        html_parser_provider: Option<Arc<dyn HtmlParserProvider>>,
        navigation_provider: Option<Arc<dyn NavigationProvider>>,
    ) -> Self {
        Self {
            inner: BlitzApplication::new(proxy.clone()),
            proxy,
            renderer_factory,
            contexts,
            net_provider,
            #[cfg(feature = "net")]
            inner_net_provider,
            html_parser_provider,
            navigation_provider,
        }
    }

    pub fn add_window(&mut self, window_config: WindowConfig<DioxusNativeWindowRenderer>) {
        self.inner.add_window(window_config);
    }

    fn _spawn_window(
        &mut self,
        config: WindowConfig<DioxusNativeWindowRenderer>,
        event_loop: &ActiveEventLoop,
    ) -> (WindowId, Arc<Window>) {
        let mut window = View::init(config, event_loop, &self.proxy);
        self.inject_window_contexts(&mut window);

        window.resume();

        let window_id = window.window_id();
        let window_arc = Arc::clone(&window.window);
        self.inner.windows.insert(window_id, window);

        (window_id, window_arc)
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

    fn create_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        vdom: UnsafeBox<dioxus_core::VirtualDom>,
        attributes: winit::window::WindowAttributes,
        reply: CreateWindowReplyOpt,
    ) {
        let mut vdom = *vdom.into_inner();

        // Make the event loop proxy available to user components.
        vdom.provide_root_context(self.proxy.clone());

        // Provide a minimal, stable API for spawning additional windows.
        vdom.provide_root_context(DioxusNativeProvider::new(self.proxy.clone()));

        #[cfg(feature = "net")]
        if let Some(inner) = &self.inner_net_provider {
            vdom.provide_root_context(Arc::clone(inner));
        }

        let contexts = &self.contexts;
        for context in contexts.iter() {
            vdom.insert_any_root_context(context());
        }
        vdom.provide_root_context(self.net_provider.clone());
        if let Some(parser) = self.html_parser_provider.clone() {
            vdom.provide_root_context(parser);
        }
        if let Some(nav) = self.navigation_provider.clone() {
            vdom.provide_root_context(nav);
        }
        let document = DioxusDocument::new(
            vdom,
            DocumentConfig {
                net_provider: Some(self.net_provider.clone()),
                html_parser_provider: self.html_parser_provider.clone(),
                navigation_provider: self.navigation_provider.clone(),
                ..Default::default()
            },
        );
        let doc = Box::new(document) as _;

        let renderer = (self.renderer_factory)();
        let config = WindowConfig::with_attributes(doc, renderer, attributes);
        let (window_id, window_arc) = self._spawn_window(config, event_loop);

        if let Some(reply) = reply {
            let _ = reply.into_inner().send((window_id, window_arc));
        }
    }

    fn handle_blitz_shell_event(&mut self, event_loop: &ActiveEventLoop, event: DioxusNativeEvent) {
        match event {
            #[cfg(all(feature = "hot-reload", debug_assertions))]
            DioxusNativeEvent::DevserverEvent(event) => match event {
                dioxus_devtools::DevserverMsg::HotReload(hotreload_message) => {
                    for window in self.inner.windows.values_mut() {
                        let doc = window.downcast_doc_mut::<DioxusDocument>();

                        // Apply changes to vdom
                        dioxus_devtools::apply_changes(&doc.vdom, &hotreload_message);

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
                if let Some(window) = self.inner.windows.get_mut(&window) {
                    let doc = window.downcast_doc_mut::<DioxusDocument>();
                    doc.create_head_element(&name, &attributes, &contents);
                    window.poll();
                }
            }

            DioxusNativeEvent::CreateDocumentWindow {
                vdom,
                attributes,
                reply,
            } => {
                self.create_window(event_loop, vdom, attributes, reply);
            }

            DioxusNativeEvent::GetWindow { window_id, reply } => {
                let window = self
                    .inner
                    .windows
                    .get(&window_id)
                    .map(|view| Arc::clone(&view.window));
                let _ = reply.into_inner().send(window);
            }

            // Suppress unused variable warning
            #[cfg(not(all(feature = "hot-reload", debug_assertions)))]
            #[allow(unreachable_patterns)]
            _ => {
                let _ = event_loop;
                let _ = &event;
            }
        }
    }
}

impl ApplicationHandler<BlitzShellEvent> for DioxusNativeApplication {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(feature = "tracing")]
        tracing::debug!("Injecting document provider into all windows");
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
                match std::sync::Arc::downcast::<DioxusNativeEvent>(event) {
                    Ok(event) => {
                        if let Ok(event) = std::sync::Arc::try_unwrap(event) {
                            self.handle_blitz_shell_event(event_loop, event);
                        } else {
                            unreachable!("Dioxus embedder event unexpectedly shared");
                        }
                    }
                    Err(_event) => {
                        unreachable!("Unhandled embedder event");
                    }
                }
            }
            event => self.inner.user_event(event_loop, event),
        }
    }
}
