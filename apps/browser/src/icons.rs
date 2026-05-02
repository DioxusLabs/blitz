use dioxus_native::prelude::*;

pub const REFRESH_ICON: Asset = asset!("../assets/icons/rotate-cw.svg");
pub const HOME_ICON: Asset = asset!("../assets/icons/house.svg");
pub const BACK_ICON: Asset = asset!("../assets/icons/arrow-left.svg");
pub const FORWARDS_ICON: Asset = asset!("../assets/icons/arrow-right.svg");
pub const MENU_ICON: Asset = asset!("../assets/icons/ellipsis-vertical.svg");
pub const EXTERNAL_LINK_ICON: Asset = asset!("../assets/icons/external-link.svg");
pub const CODE_ICON: Asset = asset!("../assets/icons/code.svg");
#[cfg(any(feature = "screenshot", feature = "capture"))]
pub const CAMERA_ICON: Asset = asset!("../assets/icons/camera.svg");

#[component]
pub fn IconButton(
    icon: Asset,
    action: Option<Callback>,
    #[props(default)] active: bool,
    #[props(default)] disabled: bool,
) -> Element {
    let class = if disabled {
        "iconbutton iconbutton--disabled"
    } else if active {
        "iconbutton active"
    } else {
        "iconbutton"
    };
    rsx!(
        div {
            class,
            onclick: move |_| {
                if disabled {
                    return;
                }
                if let Some(action) = action {
                    action(())
                }
            },
            img { class: "urlbar-icon", src: icon }
        }
    )
}
