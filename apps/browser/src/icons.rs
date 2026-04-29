use dioxus_native::prelude::*;

pub const PLUS_ICON: &str = include_str!("../assets/icons/plus.svg");
pub const REFRESH_ICON: &str = include_str!("../assets/icons/rotate-cw.svg");
pub const HOME_ICON: &str = include_str!("../assets/icons/house.svg");
pub const BACK_ICON: &str = include_str!("../assets/icons/arrow-left.svg");
pub const FORWARDS_ICON: &str = include_str!("../assets/icons/arrow-right.svg");
pub const MENU_ICON: &str = include_str!("../assets/icons/ellipsis-vertical.svg");
pub const EXTERNAL_LINK_ICON: &str = include_str!("../assets/icons/external-link.svg");
pub const CODE_ICON: &str = include_str!("../assets/icons/code.svg");
#[cfg(any(feature = "screenshot", feature = "capture"))]
pub const CAMERA_ICON: &str = include_str!("../assets/icons/camera.svg");

#[derive(Clone, Copy)]
pub struct IconColor(pub Signal<&'static str, SyncStorage>);

pub fn icon_data_url(svg: &str, color: &str) -> String {
    let mut out = String::with_capacity(svg.len().saturating_add(32));
    out.push_str("data:image/svg+xml;utf8,");
    let recolored = svg.replace("currentColor", color);
    for ch in recolored.chars() {
        match ch {
            '#' => out.push_str("%23"),
            '\n' | '\r' | '\t' => out.push(' '),
            '"' => out.push_str("%22"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            ' ' => out.push_str("%20"),
            _ => out.push(ch),
        }
    }
    out
}

#[component]
pub fn IconButton(
    icon: &'static str,
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
    let icon_color = use_context::<IconColor>();
    let src = icon_data_url(icon, &icon_color.0.read());
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
            img { class: "urlbar-icon", src }
        }
    )
}

#[component]
pub fn MenuItemIcon(icon: &'static str) -> Element {
    let icon_color = use_context::<IconColor>();
    let src = icon_data_url(icon, &icon_color.0.read());
    rsx!(img {
        class: "menu-item-icon",
        src
    })
}
