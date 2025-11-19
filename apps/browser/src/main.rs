// On Windows do NOT show a console window when opening the app
#![cfg_attr(all(not(test), target_os = "windows"), windows_subsystem = "windows")]

//! A web browser with UI powered by Dioxus Native and content rendering powered by Blitz

mod icons;
use dioxus_native::prelude::*;

use icons::IconButton;

fn main() {
    #[cfg(feature = "tracing")]
    tracing_subscriber::fmt::init();

    dioxus_native::launch(app)
}

fn app() -> Element {
    rsx!(
        body {
            title { "Blitz Browser" }
            style { {include_str!("./browser.css")} }
            div { class: "urlbar",
                IconButton { icon: icons::BACK_ICON }
                IconButton { icon: icons::FORWARDS_ICON }
                IconButton { icon: icons::REFRESH_ICON }
                IconButton { icon: icons::HOME_ICON }
                input { class: "urlbar-input", "type": "text" }
                IconButton { icon: icons::MENU_ICON }
            }
        }
    )
}
