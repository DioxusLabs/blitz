use std::{
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use blitz_traits::net::{Request, Url};
use dioxus_native::{NodeHandle, SubDocumentAttr, prelude::*};

use crate::StdNetProvider;
use crate::document_loader::{DocumentLoader, DocumentLoaderStatus, LoadedDocument};
use crate::history::{History, HistoryNav, SyncStore};

pub type TabId = u64;

static TAB_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn next_tab_id() -> TabId {
    TAB_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[derive(Store)]
pub struct Tab {
    pub id: TabId,
    pub history: SyncStore<History>,
    pub loader: Option<Rc<DocumentLoader>>,
    pub document: Option<SubDocumentAttr>,
    pub node_handle: Option<NodeHandle>,
    pub html_source: String,
    pub title: String,
}

#[store(pub)]
impl<Lens> Store<Tab, Lens> {
    fn nav_history(&self) -> SyncStore<History> {
        *self.history().read()
    }

    fn loader_rc(&self) -> Rc<DocumentLoader> {
        self.loader().cloned().expect("loader uninitialized")
    }

    fn tab_id(&self) -> TabId {
        *self.id().read()
    }

    fn navigate(&self, req: Request) {
        self.nav_history().navigate(req);
    }

    fn reload(&self) {
        self.loader_rc().reload();
    }

    fn go_back(&self) {
        self.nav_history().go_back();
    }

    fn go_forward(&self) {
        self.nav_history().go_forward();
    }

    fn apply_loaded_document(&self, loaded: LoadedDocument)
    where
        Lens: Writable,
    {
        *self.html_source().write_unchecked() = loaded.html_source;
        *self.title().write_unchecked() = loaded.title;
        *self.document().write_unchecked() = Some(loaded.document);
    }
}

pub fn open_tab(
    mut tabs: Store<Vec<Tab>>,
    url: Url,
    net_provider: Arc<StdNetProvider>,
) -> Store<Tab, impl Writable<Target = Tab> + Copy> {
    let id = next_tab_id();
    let initial_request = Request::get(url);
    let history: SyncStore<History> = Store::new_maybe_sync(History::new(initial_request.clone()));

    tabs.push(Tab {
        id,
        history,
        loader: None,
        document: None,
        node_handle: None,
        html_source: String::new(),
        title: String::new(),
    });

    let len = tabs.len();
    let tab_lens = tabs.get(len - 1).expect("just pushed");

    let loader = Rc::new(DocumentLoader::new(net_provider, history));

    *tab_lens.loader().write() = Some(loader.clone());

    tab_lens
}

pub fn active_tab(tabs: Store<Vec<Tab>>, active_id: TabId) -> Store<Tab> {
    tabs.iter()
        .find(|tab| tab.tab_id() == active_id)
        .expect("tabs vec is never empty")
        .into()
}

#[component]
pub fn TabWebView(tab: Store<Tab>, active_tab_id: Signal<TabId>) -> Element {
    let loader = tab.loader_rc();
    let loaded_document = use_resource(move || {
        let req = (*tab.nav_history().current_url().read()).clone();
        let _reload_generation = loader.reload_generation();
        let loader = loader.clone();
        async move { loader.load_document(req).await }
    });

    use_effect(move || {
        let loader = tab.loader_rc();
        let mut status = loader.status;
        match loaded_document.state().cloned() {
            UseResourceState::Pending => status.set(DocumentLoaderStatus::Loading),
            UseResourceState::Ready | UseResourceState::Stopped | UseResourceState::Paused => {
                status.set(DocumentLoaderStatus::Idle)
            }
        }
    });

    use_effect(move || {
        if loaded_document.read().is_some() {
            if let Some(loaded) = loaded_document.write_unchecked().take() {
                tab.apply_loaded_document(loaded);
            }
        }
    });

    let id = tab.tab_id();
    let document = tab.document().cloned();
    let mut node_handle_lens = tab.node_handle();

    rsx!(
        web-view {
            key: "{id}",
            class: "webview",
            style: if id == active_tab_id() { "display: block" } else { "display: none" },
            "__webview_document": document,
            onmounted: move |evt: Event<MountedData>| {
                let node_handle = evt.downcast::<NodeHandle>().unwrap();
                node_handle_lens.set(Some(node_handle.clone()));
            },
        }
    )
}

pub fn tab_title_or_url<L>(tab: Store<Tab, L>) -> String
where
    L: Copy + Readable<Target = Tab> + 'static,
{
    let title = tab.title().cloned();
    if !title.trim().is_empty() {
        return title;
    }
    tab.nav_history().current_url().read().url.to_string()
}
