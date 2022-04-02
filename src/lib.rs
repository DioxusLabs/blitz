use dioxus::{
    native_core::real_dom::{Node, RealDom},
    prelude::*,
};
use druid_shell::{Application, HotKey, Menu, SysMods, WindowBuilder};
use layout::StretchLayout;

mod layout;
mod render;
mod style;
mod util;
mod window;

type Dom = RealDom<StretchLayout, style::Style>;
type DomNode = Node<StretchLayout, style::Style>;

#[derive(Default)]
pub struct Config;

pub fn launch(root: Component<()>) {
    launch_cfg(root, Config::default())
}

pub fn launch_cfg(root: Component<()>, _cfg: Config) {
    let app = Application::new().unwrap();

    let mut file_menu = Menu::new();
    file_menu.add_item(
        0x100,
        "Exit",
        Some(&HotKey::new(SysMods::Cmd, "c")),
        true,
        false,
    );

    let mut menubar = Menu::new();
    menubar.add_dropdown(file_menu, "Application", true);

    let mut builder = WindowBuilder::new(app.clone());
    builder.set_handler(Box::new(window::WinState::new(root)));
    builder.set_title("Blitz");
    builder.set_menu(menubar);

    let window = builder.build().unwrap();
    window.show();

    app.run(None);
}
