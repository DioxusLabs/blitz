// On Windows do NOT show a console window when opening the app
#![cfg_attr(all(not(test), target_os = "windows"), windows_subsystem = "windows")]
#![allow(clippy::collapsible_if)]

//! A web browser with UI powered by Dioxus Native and content rendering powered by Blitz

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::sync::Arc;

use blitz_traits::net::Url;
use dioxus_native::{NodeHandle, WindowAttributes, prelude::*};

#[cfg(target_os = "macos")]
use winit::platform::macos::WindowAttributesMacOS;

pub(crate) type StdNetProvider = blitz_net::Provider;

mod about_pages;
mod browser_history;
#[cfg(any(feature = "screenshot", feature = "capture"))]
mod capture;
mod document_loader;
mod favicon;
#[cfg(feature = "vello")]
mod fps_overlay;
mod history;
mod icons;
mod nav;
mod status_bar;
mod tab;
mod tab_strip;
mod toolbar;
mod url_suggestions;

use about_pages::AboutPage;
use browser_history::{BrowsingHistory, HistoryService, HistoryStore, MAX_HISTORY_ENTRIES};
use status_bar::StatusBar;
use tab::{Tab, TabId, TabStoreImplExt, TabWebView, active_tab, open_tab, tab_display_title};
use tab_strip::TabStrip;
use toolbar::Toolbar;
use url_suggestions::provide_url_suggester;
#[cfg(target_os = "windows")]
use winit::platform::windows::WinIcon;

static BROWSER_UI_STYLES: Asset = asset!("../assets/browser.css");
pub(crate) const IS_MOBILE: bool = cfg!(any(target_os = "android", target_os = "ios"));

#[unsafe(no_mangle)]
#[cfg(target_os = "android")]
pub fn android_main(android_app: dioxus_native::AndroidApp) {
    dioxus_native::set_android_app(android_app);
    main()
}

fn main() {
    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt::init();
    let window_attributes = WindowAttributes::default();
    #[cfg(target_os = "windows")]
    let window_attributes = window_attributes
        .with_window_icon(WinIcon::from_resource(32512, None).map(Into::into).ok());
    #[cfg(target_os = "macos")]
    let window_attributes = window_attributes.with_platform_attributes(Box::new(
        WindowAttributesMacOS::default()
            .with_titlebar_transparent(true)
            .with_fullsize_content_view(true)
            .with_title_hidden(true)
            .with_unified_titlebar(true),
    ));

    dioxus_native::launch_cfg(app, Vec::new(), vec![Box::new(window_attributes)])
}

fn app() -> Element {
    let home_url = use_hook(|| AboutPage::NewTab.parsed_url());
    let net_provider = use_context::<Arc<StdNetProvider>>();

    let url_input_handle: Signal<Option<NodeHandle>> = use_signal(|| None);
    let url_input_value = use_signal(|| home_url.to_string());

    let history_store: HistoryStore = use_hook(HistoryStore::open);

    // Synchronous on purpose: the toolbar's URL suggestions read from this
    // store on first render, so the entries need to be present before the
    // tree mounts. The read is a single sqlite query capped at
    // MAX_HISTORY_ENTRIES rows and runs once per process.
    let browsing_history: Store<BrowsingHistory> = {
        let history_store = history_store.clone();
        use_store(move || {
            BrowsingHistory::from_entries(history_store.load_recent(MAX_HISTORY_ENTRIES))
        })
    };

    let history_service = HistoryService::new(browsing_history, history_store);
    use_context_provider(|| history_service.clone());
    provide_url_suggester(browsing_history);

    let tabs: Store<Vec<Tab>> = use_store(Vec::new);
    let mut active_tab_id: Signal<TabId> = use_hook(|| {
        let tab = open_tab(tabs, home_url.clone(), net_provider.clone());
        Signal::new(tab.tab_id())
    });

    let open_new_tab = use_callback(move |url: Url| {
        let new_id = open_tab(tabs, url, net_provider.clone());
        active_tab_id.set(new_id.tab_id());
        if let Some(handle) = url_input_handle() {
            drop(handle.set_focus(true));
        }
    });

    // HACK: Winit doesn't support "safe area" on Android yet.
    // So we just hardcode a fallback safe area.
    const TOP_PAD: &str = if cfg!(target_os = "android") {
        "30px"
    } else {
        ""
    };
    const BOTTOM_PAD: &str = if cfg!(target_os = "android") {
        "44px"
    } else {
        ""
    };

    let show_fps: Signal<bool> = use_signal(|| false);

    #[cfg(feature = "vello")]
    let fps_overlay_el = rsx!(if show_fps() {
        fps_overlay::FpsOverlay {}
    });
    #[cfg(not(feature = "vello"))]
    let fps_overlay_el = rsx!();

    let window_title = tab_display_title(active_tab(tabs, active_tab_id()));

    rsx!(
        div {
            id: "frame",
            padding_top: TOP_PAD,
            padding_bottom: BOTTOM_PAD,
            class: if IS_MOBILE { "mobile" } else { "" },
            title { "{window_title}" }
            document::Link { rel: "stylesheet", href: BROWSER_UI_STYLES }
            TabStrip {
                tabs,
                active_tab_id,
                home_url,
                open_new_tab,
            }
            Toolbar {
                url_input_handle,
                url_input_value,
                tabs,
                active_tab_id,
                open_new_tab,
                show_fps,
            }
            for tab in tabs.iter() {
                TabWebView {
                    key: "{tab.tab_id()}",
                    tab,
                    active_tab_id,
                }
            }
            {fps_overlay_el}
            StatusBar { tabs, active_tab_id }
        }
    )
}
