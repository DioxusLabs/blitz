use std::{any::Any, cell::RefCell, rc::Rc, sync::Arc};

use blitz_dom::{DocumentConfig, HtmlParserProvider};
use blitz_shell::{BlitzShellEvent, WindowConfig};
use blitz_traits::{navigation::NavigationProvider, net::NetProvider};
use dioxus_core::{ComponentFunction, Element, VirtualDom};
use winit::{event_loop::EventLoopProxy, window::WindowAttributes};

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

    pub fn build_window_with_props<P: Clone + 'static, M: 'static>(
        &self,
        component: impl ComponentFunction<P, M>,
        props: P,
    ) -> WindowConfig<DioxusNativeWindowRenderer> {
        let mut vdom = VirtualDom::new_with_props(component, props);
        for context in self.contexts.iter() {
            vdom.insert_any_root_context(context());
        }
        vdom.provide_root_context(Arc::clone(&self.net_provider));
        if let Some(parser) = &self.html_parser_provider {
            vdom.provide_root_context(Arc::clone(parser));
        }
        if let Some(navigation) = &self.navigation_provider {
            vdom.provide_root_context(Arc::clone(navigation));
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
        WindowConfig::with_attributes(
            Box::new(document) as _,
            renderer,
            self.window_attributes.clone(),
        )
    }

    pub fn build_window(
        &self,
        component: fn() -> Element,
    ) -> WindowConfig<DioxusNativeWindowRenderer> {
        self.build_window_with_props(component, ())
    }
}

pub(crate) struct DioxusWindowQueue {
    pending: RefCell<Vec<WindowConfig<DioxusNativeWindowRenderer>>>,
}

impl DioxusWindowQueue {
    pub fn new() -> Self {
        Self {
            pending: RefCell::new(Vec::new()),
        }
    }

    pub fn enqueue(&self, config: WindowConfig<DioxusNativeWindowRenderer>) {
        self.pending.borrow_mut().push(config);
    }

    pub fn drain(&self) -> Vec<WindowConfig<DioxusNativeWindowRenderer>> {
        self.pending.borrow_mut().drain(..).collect()
    }
}

#[derive(Clone)]
pub struct DioxusWindowHandle {
    proxy: EventLoopProxy<BlitzShellEvent>,
    template: Arc<DioxusWindowTemplate>,
    queue: Rc<DioxusWindowQueue>,
}

impl DioxusWindowHandle {
    pub(crate) fn new(
        proxy: EventLoopProxy<BlitzShellEvent>,
        template: Arc<DioxusWindowTemplate>,
        queue: Rc<DioxusWindowQueue>,
    ) -> Self {
        Self {
            proxy,
            template,
            queue,
        }
    }

    /// Open a window rendered by the provided component without props.
    pub fn open_window(&self, component: fn() -> Element) {
        let config = self.template.build_window(component);
        self.enqueue_and_signal(config);
    }

    /// Open a window rendered by `component` with the given props.
    pub fn open_window_with_props<P: Clone + 'static, M: 'static>(
        &self,
        component: impl ComponentFunction<P, M>,
        props: P,
    ) {
        let config = self.template.build_window_with_props(component, props);
        self.enqueue_and_signal(config);
    }

    fn enqueue_and_signal(&self, config: WindowConfig<DioxusNativeWindowRenderer>) {
        self.queue.enqueue(config);
        let _ = self.proxy.send_event(BlitzShellEvent::embedder_event(
            DioxusNativeEvent::SpawnQueuedWindows,
        ));
    }
}
