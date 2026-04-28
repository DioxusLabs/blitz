// On Windows do NOT show a console window when opening the app
#![cfg_attr(all(not(test), target_os = "windows"), windows_subsystem = "windows")]
#![allow(clippy::collapsible_if)]

//! A web browser with UI powered by Dioxus Native and content rendering powered by Blitz

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::sync::Arc;

use blitz_traits::net::Url;
use dioxus_native::{NodeHandle, prelude::*};

pub(crate) type StdNetProvider = blitz_net::Provider;

#[cfg(any(feature = "screenshot", feature = "capture"))]
mod capture;
mod document_loader;
#[cfg(feature = "vello")]
mod fps_overlay;
mod history;
mod icons;
mod status_bar;
mod tab;
mod tab_strip;
mod toolbar;

use status_bar::StatusBar;
use tab::{Tab, TabId, active_tab, tab_title_or_url};
use tab_strip::TabStrip;
use toolbar::Toolbar;

static BROWSER_UI_STYLES: Asset = asset!("../assets/browser.css");
pub(crate) const IS_MOBILE: bool = cfg!(any(target_os = "android", target_os = "ios"));
pub(crate) const HOME_URL_STR: &str = "about:newtab";

#[unsafe(no_mangle)]
#[cfg(target_os = "android")]
pub fn android_main(android_app: dioxus_native::AndroidApp) {
    dioxus_native::set_android_app(android_app);
    main()
}

fn main() {
    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt::init();
    dioxus_native::launch_cfg(app, vec![], Vec::new())
}

fn app() -> Element {
    let home_url = use_hook(|| Url::parse(HOME_URL_STR).unwrap());
    let net_provider = use_context::<Arc<StdNetProvider>>();

    let url_input_handle: Signal<Option<NodeHandle>> = use_signal(|| None);
    let url_input_value = use_signal(|| home_url.to_string());

    let mut tabs: Signal<Vec<Tab>> =
        use_hook(|| Signal::new(vec![Tab::new(home_url.clone(), net_provider.clone())]));
    let mut active_tab_id: Signal<TabId> =
        use_hook(|| Signal::new(tabs.read().first().map(|t| t.id).unwrap_or(0)));

    let open_new_tab = use_callback(move |url: Url| {
        let new_tab = Tab::new(url, net_provider.clone());
        let new_id = new_tab.id;
        tabs.write().push(new_tab);
        active_tab_id.set(new_id);
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

    let window_title = tab_title_or_url(&active_tab(&tabs, active_tab_id()));

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
                home_url: home_url.clone(),
                open_new_tab,
            }
            Toolbar {
                url_input_handle,
                url_input_value,
                tabs,
                active_tab_id,
                show_fps,
            }
            for tab in tabs() {
                {
                    let mut tab_node_handle = tab.node_handle;
                    rsx!(
                        web-view {
                            key: "{tab.id}",
                            class: "webview",
                            style: if tab.id == active_tab_id() { "display: block" } else { "display: none" },
                            "__webview_document": tab.document.cloned(),
                            onmounted: move |evt: Event<MountedData>| {
                                let node_handle = evt.downcast::<NodeHandle>().unwrap();
                                *tab_node_handle.write() = Some(node_handle.clone());
                            },
                        }
                    )
                }
            }
            {fps_overlay_el}
            StatusBar { tabs, active_tab_id }
        }
    )
}
