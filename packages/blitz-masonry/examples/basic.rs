#![windows_subsystem = "windows"]

use blitz_masonry::{Element, Text};
use masonry::app_driver::{AppDriver, DriverCtx};
use masonry::widget::RootWidget;
use masonry::{Action, WidgetId};
use winit::window::WindowAttributes;

struct Driver;

impl AppDriver for Driver {
    fn on_action(&mut self, ctx: &mut DriverCtx<'_>, widget_id: WidgetId, action: Action) {}
}

pub fn main() {
    let mut h1 = Element::new("h1");
    h1.append_child(Text::new("Hello, World!"));

    let mut div = Element::new("div");
    div.font_size = Some(100.0);
    div.append_child(h1);

    masonry::event_loop_runner::run(
        masonry::event_loop_runner::EventLoop::with_user_event(),
        WindowAttributes::default(),
        RootWidget::new(div),
        Driver,
    )
    .unwrap();
}
