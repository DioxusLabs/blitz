#![windows_subsystem = "windows"]

use masonry::app_driver::{AppDriver, DriverCtx};
use masonry::dpi::LogicalSize;
use masonry::widget::{Flex, Label, RootWidget};
use masonry::{Action, WidgetId};
use winit::window::Window;

struct Driver;

impl AppDriver for Driver {
    fn on_action(&mut self, ctx: &mut DriverCtx<'_>, widget_id: WidgetId, action: Action) {}
}

pub fn main() {
    let label = Label::new("Hello").with_text_size(32.0);

    let main_widget = Flex::column().with_child(label);

    let window_size = LogicalSize::new(600.0, 400.0);
    let window_attributes = Window::default_attributes()
        .with_title("Blitz")
        .with_min_inner_size(window_size);

    masonry::event_loop_runner::run(
        masonry::event_loop_runner::EventLoop::with_user_event(),
        window_attributes,
        RootWidget::new(main_widget),
        Driver,
    )
    .unwrap();
}
