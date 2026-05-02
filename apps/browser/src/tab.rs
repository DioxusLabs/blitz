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
use crate::about_pages::{AboutPage, AboutPageView};
use crate::browser_history::{BrowsingHistory, BrowsingHistoryStoreImplExt, HistoryEntry};
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
    pub favicon_url: Option<Url>,
}

#[store(pub)]
impl<Lens> Store<Tab, Lens> {
    fn nav_history(&self) -> SyncStore<History> {
        *self.history().read()
    }

    fn loader_rc(&self) -> Rc<DocumentLoader> {
        // `open_tab` always assigns Some(loader) immediately after pushing the
        // tab, so by the time any view code can call this the loader is set.
        #[allow(clippy::expect_used)]
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

    // Chrome state (title, favicon) is owned here for real (loaded) pages and
    // by `apply_about_chrome` for about pages. Exactly one of the two runs per
    // navigation, gated on whether the URL parsed as an `AboutPage`.
    fn apply_loaded_document(&self, loaded: LoadedDocument)
    where
        Lens: Writable,
    {
        *self.html_source().write_unchecked() = loaded.html_source;
        *self.title().write_unchecked() = loaded.title;
        *self.favicon_url().write_unchecked() = loaded.favicon_url;
        *self.document().write_unchecked() = Some(loaded.document);
    }

    // Sibling of `apply_loaded_document` for chrome (about:) pages. About URLs
    // never go through the document loader, so this is the only place their
    // title is set and the only place a stale favicon from a previous real
    // page gets cleared.
    fn apply_about_chrome(&self, page: AboutPage)
    where
        Lens: Writable,
    {
        *self.title().write_unchecked() = page.title().to_string();
        *self.favicon_url().write_unchecked() = None;
    }
}

pub fn open_tab(
    mut tabs: Store<Vec<Tab>>,
    url: Url,
    net_provider: Arc<StdNetProvider>,
) -> Store<Tab, impl Writable<Target = Tab> + Copy> {
    let id = next_tab_id();
    let initial_request = Request::get(url);
    let history: SyncStore<History> = Store::new_maybe_sync(History::new(initial_request));

    tabs.push(Tab {
        id,
        history,
        loader: None,
        document: None,
        node_handle: None,
        html_source: String::new(),
        title: String::new(),
        favicon_url: None,
    });

    // We just pushed; the last element is the tab we want.
    #[allow(clippy::expect_used)]
    let tab_lens = tabs.iter().last().expect("just pushed");

    let loader = Rc::new(DocumentLoader::new(net_provider, history));

    *tab_lens.loader().write() = Some(loader);

    tab_lens
}

pub fn active_tab(tabs: Store<Vec<Tab>>, active_id: TabId) -> Store<Tab> {
    // The app always keeps at least one tab open and only switches `active_id`
    // to ids that exist; closing a tab updates `active_id` first.
    #[allow(clippy::expect_used)]
    tabs.iter()
        .find(|tab| tab.tab_id() == active_id)
        .expect("tabs vec is never empty")
        .into()
}

#[component]
pub fn TabWebView(
    tab: Store<Tab>,
    active_tab_id: Signal<TabId>,
    browsing_history: Store<BrowsingHistory>,
) -> Element {
    let about = use_memo(move || AboutPage::from_url(&tab.nav_history().current_url().read().url));

    let loader = tab.loader_rc();
    let loaded_document = use_resource(move || {
        let req = (*tab.nav_history().current_url().read()).clone();
        let _reload_generation = loader.reload_generation();
        let loader = loader.clone();
        let is_about = about().is_some();
        async move {
            if is_about {
                None
            } else {
                Some(loader.load_document(req).await)
            }
        }
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
            if let Some(loaded) = loaded_document.write_unchecked().take().flatten() {
                // Only successful loads count as a visit. Synthesized 404 /
                // network-error pages parse a title (so the tab strip shows
                // something sensible) but shouldn't pollute history.
                if !loaded.is_error {
                    let url = tab.nav_history().current_url().read().url.clone();
                    let title = display_title(&loaded.title, &url);
                    let favicon = loaded.favicon_url.clone();
                    browsing_history.record_visit(HistoryEntry::new(url, title, favicon));
                }
                tab.apply_loaded_document(loaded);
            }
        }
    });

    use_effect(move || {
        if let Some(page) = about() {
            tab.apply_about_chrome(page);
        }
    });

    let id = tab.tab_id();
    let document = tab.document().cloned();
    let mut node_handle_lens = tab.node_handle();
    let visibility = if id == active_tab_id() {
        "display: block"
    } else {
        "display: none"
    };

    let on_navigate = use_callback(move |req: Request| {
        tab.navigate(req);
    });

    rsx!(
        if let Some(page) = about() {
            div {
                key: "{id}",
                class: "tab-content",
                style: visibility,
                AboutPageView { page, on_navigate, browsing_history }
            }
        } else {
            web-view {
                key: "{id}",
                class: "tab-content",
                style: visibility,
                "__webview_document": document,
                onmounted: move |evt: Event<MountedData>| {
                    let Some(node_handle) = evt.downcast::<NodeHandle>() else { return };
                    node_handle_lens.set(Some(node_handle.clone()));
                },
            }
        }
    )
}

pub fn tab_display_title<L>(tab: Store<Tab, L>) -> String
where
    L: Copy + Readable<Target = Tab> + 'static,
{
    let title = tab.title().cloned();
    let url = tab.nav_history().current_url().read().url.clone();
    display_title(&title, &url)
}

// Single source of truth for the "use the page title, fall back to the URL
// when the title is empty/whitespace" rule. Both the tab strip (reading the
// stored title) and history recording (using the freshly parsed title) need
// the same behavior.
fn display_title(title: &str, url: &Url) -> String {
    if title.trim().is_empty() {
        url.to_string()
    } else {
        title.to_string()
    }
}

#[component]
pub fn Favicon(url: Option<Url>, class: &'static str) -> Element {
    rsx! {
        if let Some(url) = url {
            img {
                class: class,
                src: "{url}",
                width: "16",
                height: "16",
                alt: "",
            }
        }
    }
}
