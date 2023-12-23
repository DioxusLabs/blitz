use dioxus::core::{Component, VirtualDom};
use glazier::*;
use tao::dpi::PhysicalSize;
use vello::Scene;

use crate::{styling::RealDom, viewport::Viewport, Document};

const WIDTH: usize = 2048;
const HEIGHT: usize = 1536;

pub fn launch_glazier(f: Component<()>, cfg: crate::Config) {
    // Turn on the runtime and enter it
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let _ = rt.enter();

    // Spin up the virtualdom
    // We're going to need to hit it with a special waker
    let mut virtualdom = VirtualDom::new_with_props(f, ());
    _ = virtualdom.rebuild();
    let markup = dioxus_ssr::render(&virtualdom);

    let app = Application::new().unwrap();
    let window = glazier::WindowBuilder::new(app.clone())
        .resizable(true)
        .size((WIDTH as f64 / 2., HEIGHT as f64 / 2.).into())
        .handler(Box::new(WindowState::new()))
        .build()
        .unwrap();

    // Set up the blitz drawing system
    // todo: this won't work on ios - blitz creation has to be deferred until the event loop as started
    let dom = RealDom::new(markup);

    let size = window.get_size();
    let mut viewport = Viewport::new(PhysicalSize {
        height: size.height as _,
        width: size.width as _,
    });
    viewport.set_hidpi_scale(window.get_scale().map(|f| f.x()).unwrap_or(1.0) as _);

    let mut blitz = rt.block_on(Document::from_window(&window, dom, viewport));
    let mut scene = Scene::new();

    // add default styles, resolve layout and styles
    for ss in cfg.stylesheets {
        blitz.add_stylesheet(&ss);
    }

    blitz.resolve();
    blitz.render(&mut scene);

    window.show();
    app.run(None);
}

struct WindowState {}

impl WindowState {
    fn new() -> Self {
        Self {}
    }
}

impl WinHandler for WindowState {
    fn connect(&mut self, handle: &WindowHandle) {}

    fn prepare_paint(&mut self) {}

    // comes with blitting!
    fn paint(&mut self, invalid: &Region) {}

    fn as_any(&mut self) -> &mut dyn std::any::Any {
        todo!()
    }

    fn size(&mut self, size: kurbo::Size) {}

    fn scale(&mut self, scale: Scale) {}

    fn rebuild_resources(&mut self) {}

    fn command(&mut self, id: u32) {}

    fn save_as(&mut self, token: FileDialogToken, file: Option<FileInfo>) {}

    fn open_file(&mut self, token: FileDialogToken, file: Option<FileInfo>) {}

    fn open_files(&mut self, token: FileDialogToken, files: Vec<FileInfo>) {}

    fn key_down(&mut self, event: &KeyEvent) -> bool {
        false
    }

    fn key_up(&mut self, event: &KeyEvent) {}

    fn acquire_input_lock(
        &mut self,
        token: TextFieldToken,
        mutable: bool,
    ) -> Box<dyn text::InputHandler> {
        panic!("acquire_input_lock was called on a WinHandler that did not expect text input.")
    }

    fn release_input_lock(&mut self, token: TextFieldToken) {
        panic!("release_input_lock was called on a WinHandler that did not expect text input.")
    }

    fn zoom(&mut self, delta: f64) {}

    fn wheel(&mut self, event: &PointerEvent) {}

    fn pointer_move(&mut self, event: &PointerEvent) {}

    fn pointer_down(&mut self, event: &PointerEvent) {}

    fn pointer_up(&mut self, event: &PointerEvent) {}

    fn pointer_leave(&mut self) {}

    fn timer(&mut self, token: TimerToken) {}

    fn got_focus(&mut self) {}

    fn lost_focus(&mut self) {}

    fn request_close(&mut self) {}

    fn destroy(&mut self) {}

    fn idle(&mut self, token: IdleToken) {}
}
