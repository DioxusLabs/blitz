#![windows_subsystem = "windows"]

use blitz_masonry::DocumentWidget;
use masonry::app_driver::{AppDriver, DriverCtx};
use masonry::widget::RootWidget;
use masonry::{Action, WidgetId};
use winit::window::WindowAttributes;

struct Driver;

impl AppDriver for Driver {
    fn on_action(&mut self, ctx: &mut DriverCtx<'_>, widget_id: WidgetId, action: Action) {}
}

pub fn main() {
    let widget = DocumentWidget::from_html("<h1>Hello World!</h1>");

    masonry::event_loop_runner::run(
        masonry::event_loop_runner::EventLoop::with_user_event(),
        WindowAttributes::default(),
        RootWidget::new(widget),
        Driver,
    )
    .unwrap();
}
