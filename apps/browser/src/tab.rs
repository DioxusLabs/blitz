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
use crate::config::ConfigStore;
use crate::document_loader::{DocumentLoader, LoadTrigger, LoadTriggerSignal};
use crate::history::{History, HistoryNav, SyncStore};

pub type TabId = u64;

static TAB_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn next_tab_id() -> TabId {
    TAB_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone)]
pub struct Tab {
    pub id: TabId,
    pub history: SyncStore<History>,
    pub load_trigger: LoadTriggerSignal,
    pub loader: Rc<DocumentLoader>,
    pub document: Signal<Option<SubDocumentAttr>>,
    pub node_handle: Signal<Option<NodeHandle>>,
    pub html_source: Signal<String>,
    pub title: Signal<String>,
}

impl PartialEq for Tab {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Tab {
    pub fn new(url: Url, net_provider: Arc<StdNetProvider>, config: Arc<ConfigStore>) -> Self {
        let id = next_tab_id();
        let history: SyncStore<History> = Store::new_maybe_sync(History::new(url.clone()));
        // BackForward so the initial load doesn't add a duplicate history entry.
        let load_trigger: LoadTriggerSignal =
            Signal::new_maybe_sync(LoadTrigger::BackForward(Request::get(url)));
        let html_source: Signal<String> = Signal::new(String::new());
        let title: Signal<String> = Signal::new(String::new());
        let loader = Rc::new(DocumentLoader::new(
            net_provider,
            config,
            history,
            load_trigger,
            html_source,
            title,
        ));
        let document = loader.doc;
        Tab {
            id,
            history,
            load_trigger,
            loader,
            document,
            node_handle: Signal::new(None),
            html_source,
            title,
        }
    }
}

pub fn active_tab(tabs: &Signal<Vec<Tab>>, active_id: TabId) -> Tab {
    let tabs_ref = tabs.read();
    #[allow(clippy::expect_used)] // tabs is never empty: always initialised with one tab
    tabs_ref
        .iter()
        .find(|t| t.id == active_id)
        .or_else(|| tabs_ref.first())
        .expect("tabs vec is never empty")
        .clone()
}

pub fn tab_title_or_url(tab: &Tab) -> String {
    let title = tab.title.read();
    if !title.trim().is_empty() {
        return title.clone();
    }
    tab.history.current_url().read().url.to_string()
}
