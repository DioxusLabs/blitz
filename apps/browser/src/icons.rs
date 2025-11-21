use dioxus_native::prelude::*;

pub const REFRESH_ICON: Asset = asset!("/assets/rotate-cw.svg");
pub const HOME_ICON: Asset = asset!("/assets/house.svg");
pub const BACK_ICON: Asset = asset!("/assets/arrow-left.svg");
pub const FORWARDS_ICON: Asset = asset!("/assets/arrow-right.svg");
pub const MENU_ICON: Asset = asset!("/assets/ellipsis-vertical.svg");

#[component]
pub fn IconButton(icon: Asset, action: Option<Callback>) -> Element {
    rsx!(
        div {
            class: "iconbutton",
            onclick: move |_| {
                if let Some(action) = action {
                    action(())
                }
            },
            img { class: "urlbar-icon", src: icon }
        }
    )
}
