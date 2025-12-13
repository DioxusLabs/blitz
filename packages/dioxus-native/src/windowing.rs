use std::{any::Any, cell::RefCell, rc::Rc, sync::Arc};

use blitz_dom::{DocumentConfig, HtmlParserProvider};
use blitz_shell::{BlitzShellEvent, WindowConfig};
use blitz_traits::{navigation::NavigationProvider, net::NetProvider};
use dioxus_core::{ComponentFunction, Element, VirtualDom};
use winit::{
    event_loop::EventLoopProxy,
    window::{WindowAttributes, WindowId},
};

use crate::{DioxusDocument, DioxusNativeEvent, DioxusNativeWindowRenderer};

pub struct DioxusWindowTemplate {
    contexts: Arc<Vec<Box<dyn Fn() -> Box<dyn Any> + Send + Sync>>>,
    renderer_factory: Arc<dyn Fn() -> DioxusNativeWindowRenderer + Send + Sync>,
    window_attributes: WindowAttributes,
    net_provider: Arc<dyn NetProvider>,
    html_parser_provider: Option<Arc<dyn HtmlParserProvider>>,
    navigation_provider: Option<Arc<dyn NavigationProvider>>,
}

impl DioxusWindowTemplate {
    pub fn new(
        contexts: Arc<Vec<Box<dyn Fn() -> Box<dyn Any> + Send + Sync>>>,
        renderer_factory: Arc<dyn Fn() -> DioxusNativeWindowRenderer + Send + Sync>,
        window_attributes: WindowAttributes,
        net_provider: Arc<dyn NetProvider>,
        html_parser_provider: Option<Arc<dyn HtmlParserProvider>>,
        navigation_provider: Option<Arc<dyn NavigationProvider>>,
    ) -> Self {
        Self {
            contexts,
            renderer_factory,
            window_attributes,
            net_provider,
            html_parser_provider,
            navigation_provider,
        }
    }

    pub fn build_window(
        &self,
        component: fn() -> Element,
        attributes_override: Option<WindowAttributes>,
    ) -> QueuedWindow {
        self.build_window_with_props(component, (), attributes_override)
    }

    pub fn build_window_with_props<P: Clone + 'static, M: 'static>(
        &self,
        component: impl ComponentFunction<P, M>,
        props: P,
        attributes_override: Option<WindowAttributes>,
    ) -> QueuedWindow {
        let attributes = attributes_override.unwrap_or_else(|| self.window_attributes.clone());
        let title = attributes.title.clone();

        let mut vdom = VirtualDom::new_with_props(component, props);
        for context in self.contexts.iter() {
            vdom.insert_any_root_context(context());
        }
        vdom.provide_root_context(Arc::clone(&self.net_provider));
        if let Some(parser) = &self.html_parser_provider {
            vdom.provide_root_context(Arc::clone(parser));
        }
        if let Some(nav) = &self.navigation_provider {
            vdom.provide_root_context(Arc::clone(nav));
        }

        let document = DioxusDocument::new(
            vdom,
            DocumentConfig {
                net_provider: Some(Arc::clone(&self.net_provider)),
                html_parser_provider: self.html_parser_provider.clone(),
                navigation_provider: self.navigation_provider.clone(),
                ..Default::default()
            },
        );

        let renderer = (self.renderer_factory)();
        let config = WindowConfig::with_attributes(Box::new(document) as _, renderer, attributes);
        QueuedWindow { config, title }
    }
}

pub struct QueuedWindow {
    pub config: WindowConfig<DioxusNativeWindowRenderer>,
    pub title: String,
}

#[derive(Clone, Debug)]
pub struct DioxusWindowInfo {
    pub id: WindowId,
    pub title: String,
}

pub struct DioxusWindowQueue {
    pending: RefCell<Vec<QueuedWindow>>,
}

impl DioxusWindowQueue {
    pub fn new() -> Self {
        Self {
            pending: RefCell::new(Vec::new()),
        }
    }

    pub fn enqueue(&self, window: QueuedWindow) {
        self.pending.borrow_mut().push(window);
    }

    pub fn drain(&self) -> Vec<QueuedWindow> {
        self.pending.borrow_mut().drain(..).collect()
    }
}

#[derive(Clone)]
pub struct DioxusWindowHandle {
    proxy: EventLoopProxy<BlitzShellEvent>,
    template: Arc<DioxusWindowTemplate>,
    queue: Rc<DioxusWindowQueue>,
    registry: Rc<RefCell<Vec<DioxusWindowInfo>>>,
}

impl DioxusWindowHandle {
    pub(crate) fn new(
        proxy: EventLoopProxy<BlitzShellEvent>,
        template: Arc<DioxusWindowTemplate>,
        queue: Rc<DioxusWindowQueue>,
        registry: Rc<RefCell<Vec<DioxusWindowInfo>>>,
    ) -> Self {
        Self {
            proxy,
            template,
            queue,
            registry,
        }
    }

    pub fn open_window(&self, component: fn() -> Element) {
        self.open_window_with_attributes(component, None);
    }

    pub fn open_window_with_attributes(
        &self,
        component: fn() -> Element,
        attributes: Option<WindowAttributes>,
    ) {
        let queued = self.template.build_window(component, attributes);
        self.enqueue_and_signal(queued);
    }

    pub fn open_window_with_props<P: Clone + 'static, M: 'static>(
        &self,
        component: impl ComponentFunction<P, M>,
        props: P,
    ) {
        self.open_window_with_props_and_attributes(component, props, None);
    }

    pub fn open_window_with_props_and_attributes<P: Clone + 'static, M: 'static>(
        &self,
        component: impl ComponentFunction<P, M>,
        props: P,
        attributes: Option<WindowAttributes>,
    ) {
        let queued = self
            .template
            .build_window_with_props(component, props, attributes);
        self.enqueue_and_signal(queued);
    }

    pub fn list_windows(&self) -> Vec<DioxusWindowInfo> {
        self.registry.borrow().clone()
    }

    pub fn focus_window(&self, window_id: WindowId) {
        let _ = self.proxy.send_event(BlitzShellEvent::embedder_event(
            DioxusNativeEvent::FocusWindow { window_id },
        ));
    }

    pub fn set_window_title(&self, window_id: WindowId, title: impl Into<String>) {
        let title = title.into();
        let _ = self.proxy.send_event(BlitzShellEvent::embedder_event(
            DioxusNativeEvent::SetWindowTitle { window_id, title },
        ));
    }

    fn enqueue_and_signal(&self, window: QueuedWindow) {
        self.queue.enqueue(window);
        let _ = self.proxy.send_event(BlitzShellEvent::embedder_event(
            DioxusNativeEvent::SpawnQueuedWindows,
        ));
    }
}
