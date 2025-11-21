// On Windows do NOT show a console window when opening the app
#![cfg_attr(all(not(test), target_os = "windows"), windows_subsystem = "windows")]

//! A web browser with UI powered by Dioxus Native and content rendering powered by Blitz

use std::future::Future;
use std::sync::Mutex;
use std::sync::{Arc, atomic::AtomicUsize, atomic::Ordering as Ao};

use dioxus_native::prelude::*;


type StdNetProvider = blitz_net::Provider<blitz_dom::net::Resource>;

mod icons;
use icons::IconButton;

fn main() {
    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt::init();

    dioxus_native::launch(app)
}

fn app() -> Element {
    let mut url_input_value = use_signal(|| String::from("https://startpage.com"));
    let mut url: Signal<Option<String>> = use_signal(|| None);

    let net_provider = use_context::<Arc<StdNetProvider>>();

    rsx!(
        body {
            title { "Blitz Browser" }
            style { {include_str!("./browser.css")} }
            div { class: "urlbar",
                IconButton { icon: icons::BACK_ICON }
                IconButton { icon: icons::FORWARDS_ICON }
                IconButton { icon: icons::REFRESH_ICON }
                IconButton { icon: icons::HOME_ICON }
                input {
                    class: "urlbar-input",
                    "type": "text",
                    name: "url",
                    value: url_input_value(),
                    onkeydown: move |evt| {
                        if evt.key() == Key::Enter {
                            evt.prevent_default();
                            *url.write() = Some(url_input_value());
                        }
                    },
                    oninput: move |evt| {
                        *url_input_value.write() = evt.value()
                    },
                }
                IconButton { icon: icons::MENU_ICON }
            }
        }
    )
}

fn load_page() {
    // TODO: load document
}
