//! A window containing a code editor, using winit and AnyRender directly (no Blitz document).
//!
//! Usage: `cargo run -p parley_code_editor --example editor [--cpu] [--wrap] [file]`

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyrender::WindowRenderer;
use anyrender_vello::VelloWindowRenderer;
use anyrender_vello_cpu::VelloCpuWindowRenderer;
use kurbo::{Affine, Point, Size};
use parley_code_editor::{
    CodeEditor, EditorInput, EditorResponse, ImeInput, Key, KeyInput, Modifiers, PointerButton,
    ScrollDelta, TextContext,
};
use winit::application::ApplicationHandler;
use winit::cursor::{Cursor, CursorIcon};
use winit::data_transfer::{DataTransferSendBuilder, TypeHint};
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event::{ButtonSource, ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, AsyncRequestSerial, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};
use winit::window::{
    ImeCapabilities, ImeEnableRequest, ImeRequest, ImeRequestData, Window, WindowAttributes,
    WindowId,
};

mod common;

/// Maximum time and distance between clicks for them to count as a double/triple click
const MULTI_CLICK_TIME: Duration = Duration::from_millis(500);
const MULTI_CLICK_DISTANCE: f64 = 4.0;

enum Renderer {
    Gpu(Box<VelloWindowRenderer>),
    Cpu(Box<VelloCpuWindowRenderer>),
}

impl Renderer {
    fn resume(&mut self, window: Arc<dyn Window>) {
        let size = window.surface_size();
        let handle = Arc::new(window);
        match self {
            Renderer::Gpu(r) => r.resume(handle, size.width, size.height, || {}),
            Renderer::Cpu(r) => r.resume(handle, size.width, size.height, || {}),
        }
        match self {
            Renderer::Gpu(r) => r.complete_resume(),
            Renderer::Cpu(r) => r.complete_resume(),
        };
    }

    fn suspend(&mut self) {
        match self {
            Renderer::Gpu(r) => r.suspend(),
            Renderer::Cpu(r) => r.suspend(),
        }
    }

    fn set_size(&mut self, width: u32, height: u32) {
        match self {
            Renderer::Gpu(r) => r.set_size(width, height),
            Renderer::Cpu(r) => r.set_size(width, height),
        }
    }
}

struct App {
    editor: CodeEditor,
    text_cx: TextContext,
    renderer: Renderer,
    window: Option<Arc<dyn Window>>,
    modifiers: ModifiersState,
    pointer_pos: Point,
    /// Time, position and count of the last click, to detect double/triple clicks
    last_click: Option<(Instant, Point, u8)>,
    pending_paste: Option<AsyncRequestSerial>,
    ime_enabled: bool,
    /// When to send the editor its next `EditorInput::Timer`
    timer_deadline: Option<Instant>,
}

impl App {
    fn scale(&self) -> f64 {
        self.window.as_ref().map_or(1.0, |w| w.scale_factor())
    }

    fn mods(&self) -> Modifiers {
        Modifiers {
            shift: self.modifiers.shift_key(),
            ctrl: self.modifiers.control_key(),
            alt: self.modifiers.alt_key(),
            meta: self.modifiers.meta_key(),
        }
    }

    fn to_logical(&self, pos: PhysicalPosition<f64>) -> Point {
        let scale = self.scale();
        Point::new(pos.x / scale, pos.y / scale)
    }

    fn send(&mut self, event_loop: &dyn ActiveEventLoop, input: EditorInput) {
        let resp = self.editor.handle_input(&mut self.text_cx, input);
        self.apply_response(event_loop, resp);
    }

    fn apply_response(&mut self, event_loop: &dyn ActiveEventLoop, resp: EditorResponse) {
        let Some(window) = self.window.clone() else {
            return;
        };
        if resp.redraw {
            window.request_redraw();
        }
        if let Some(delay) = resp.timer {
            self.timer_deadline = Some(Instant::now() + delay);
        }
        if let Some(text) = resp.set_clipboard {
            let data = DataTransferSendBuilder::new(text)
                .with_type(TypeHint::Plaintext, |text, _| Some(text.clone()))
                .build();
            if let Err(err) = event_loop.set_clipboard(data) {
                eprintln!("Failed to set clipboard: {err}");
            }
        }
        if resp.request_clipboard
            && let Ok(Some(id)) = event_loop.clipboard()
            && let Ok(serial) = event_loop.fetch_data_transfer(id, &TypeHint::Plaintext)
        {
            self.pending_paste = Some(serial);
        }
        let cursor_area = resp.ime_cursor_area.map(|rect| {
            (
                LogicalPosition::new(rect.x0, rect.y0).into(),
                LogicalSize::new(rect.width(), rect.height()).into(),
            )
        });
        match resp.ime_enabled {
            Some(true) => {
                let (position, size) = cursor_area.unwrap_or((
                    LogicalPosition::new(0.0, 0.0).into(),
                    LogicalSize::new(1.0, 1.0).into(),
                ));
                let request = ImeEnableRequest::new(
                    ImeCapabilities::new().with_cursor_area(),
                    ImeRequestData::default().with_cursor_area(position, size),
                )
                .unwrap();
                let _ = window.request_ime_update(ImeRequest::Enable(request));
                self.ime_enabled = true;
            }
            Some(false) => {
                let _ = window.request_ime_update(ImeRequest::Disable);
                self.ime_enabled = false;
            }
            None => {
                if let Some((position, size)) = cursor_area
                    && self.ime_enabled
                {
                    let data = ImeRequestData::default().with_cursor_area(position, size);
                    let _ = window.request_ime_update(ImeRequest::Update(data));
                }
            }
        }
    }

    fn click_count(&mut self, pos: Point) -> u8 {
        let now = Instant::now();
        let count = match self.last_click {
            Some((time, last_pos, count))
                if now - time < MULTI_CLICK_TIME
                    && (pos - last_pos).hypot() < MULTI_CLICK_DISTANCE =>
            {
                count % 3 + 1
            }
            _ => 1,
        };
        self.last_click = Some((now, pos, count));
        count
    }

    fn redraw(&mut self) {
        let scale = self.scale();
        let editor = &mut self.editor;
        let cx = &mut self.text_cx;
        let transform = Affine::scale(scale);
        match &mut self.renderer {
            Renderer::Gpu(r) => r.render(|scene| editor.paint(cx, scene, transform)),
            Renderer::Cpu(r) => r.render(|scene| editor.paint(cx, scene, transform)),
        }
    }
}

fn convert_key(key: &WinitKey) -> Key {
    match key {
        WinitKey::Character(c) => Key::Character(c.to_string()),
        WinitKey::Named(named) => match named {
            NamedKey::ArrowLeft => Key::ArrowLeft,
            NamedKey::ArrowRight => Key::ArrowRight,
            NamedKey::ArrowUp => Key::ArrowUp,
            NamedKey::ArrowDown => Key::ArrowDown,
            NamedKey::Home => Key::Home,
            NamedKey::End => Key::End,
            NamedKey::PageUp => Key::PageUp,
            NamedKey::PageDown => Key::PageDown,
            NamedKey::Backspace => Key::Backspace,
            NamedKey::Delete => Key::Delete,
            NamedKey::Enter => Key::Enter,
            NamedKey::Tab => Key::Tab,
            NamedKey::Escape => Key::Escape,
            _ => Key::Other,
        },
        _ => Key::Other,
    }
}

fn convert_ime(ime: Ime) -> ImeInput {
    match ime {
        Ime::Enabled => ImeInput::Enabled,
        Ime::Preedit(text, cursor) => ImeInput::Preedit { text, cursor },
        Ime::Commit(text) => ImeInput::Commit(text),
        Ime::DeleteSurrounding {
            before_bytes,
            after_bytes,
        } => ImeInput::DeleteSurrounding {
            before_bytes,
            after_bytes,
        },
        Ime::Disabled => ImeInput::Disabled,
    }
}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        let window = match &self.window {
            Some(window) => window.clone(),
            None => {
                let attrs = WindowAttributes::default()
                    .with_title("Code editor")
                    .with_surface_size(LogicalSize::new(1000.0, 700.0));
                let window: Arc<dyn Window> = Arc::from(event_loop.create_window(attrs).unwrap());
                window.set_cursor(Cursor::Icon(CursorIcon::Text));
                self.window = Some(window.clone());
                window
            }
        };
        self.renderer.resume(window.clone());

        let size = window.surface_size().to_logical(window.scale_factor());
        self.editor.set_size(Size::new(size.width, size.height));
        self.send(event_loop, EditorInput::Focus(true));
    }

    fn destroy_surfaces(&mut self, _event_loop: &dyn ActiveEventLoop) {
        self.renderer.suspend();
    }

    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self
            .timer_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.timer_deadline = None;
            self.send(event_loop, EditorInput::Timer);
        }
        event_loop.set_control_flow(match self.timer_deadline {
            Some(deadline) => ControlFlow::WaitUntil(deadline),
            None => ControlFlow::Wait,
        });
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.renderer.suspend();
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::SurfaceResized(size) => {
                self.renderer.set_size(size.width, size.height);
                let logical = size.to_logical(self.scale());
                self.editor
                    .set_size(Size::new(logical.width, logical.height));
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(window) = &self.window {
                    let logical = window.surface_size().to_logical(scale_factor);
                    self.editor
                        .set_size(Size::new(logical.width, logical.height));
                    window.request_redraw();
                }
            }
            WindowEvent::Focused(focused) => self.send(event_loop, EditorInput::Focus(focused)),
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let input = KeyInput {
                    key: convert_key(&event.logical_key),
                    mods: self.mods(),
                    text: event.text.map(|text| text.to_string()),
                };
                self.send(event_loop, EditorInput::Key(input));
            }
            WindowEvent::Ime(ime) => self.send(event_loop, EditorInput::Ime(convert_ime(ime))),
            WindowEvent::PointerMoved { position, .. } => {
                self.pointer_pos = self.to_logical(position);
                let pos = self.pointer_pos;
                self.send(event_loop, EditorInput::PointerMove { pos });
            }
            WindowEvent::PointerButton {
                button: ButtonSource::Mouse(button),
                state,
                position,
                ..
            } => {
                let pos = self.to_logical(position);
                let button = match button {
                    MouseButton::Left => PointerButton::Primary,
                    MouseButton::Right => PointerButton::Secondary,
                    _ => PointerButton::Middle,
                };
                let input = match state {
                    ElementState::Pressed => EditorInput::PointerDown {
                        pos,
                        button,
                        mods: self.mods(),
                        click_count: self.click_count(pos),
                    },
                    ElementState::Released => EditorInput::PointerUp { pos, button },
                };
                self.send(event_loop, input);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let delta = match delta {
                    MouseScrollDelta::LineDelta(x, y) => ScrollDelta::Lines(x as f64, y as f64),
                    MouseScrollDelta::PixelDelta(pos) => {
                        let pos = self.to_logical(pos);
                        ScrollDelta::Pixels(pos.x, pos.y)
                    }
                    _ => return,
                };
                self.send(event_loop, EditorInput::Wheel { delta });
            }
            WindowEvent::DataTransferReceived { serial, value, .. }
                if self.pending_paste == Some(serial) =>
            {
                let text = match value.try_as_string() {
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => return,
                    result => result,
                };
                self.pending_paste = None;
                if let Ok(text) = text {
                    self.send(event_loop, EditorInput::Paste(text));
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let mut use_cpu = false;
    let mut wrap = false;
    let mut path = None;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--cpu" => use_cpu = true,
            "--wrap" => wrap = true,
            _ => path = Some(arg),
        }
    }
    let renderer = if use_cpu {
        Renderer::Cpu(Box::new(VelloCpuWindowRenderer::new()))
    } else {
        Renderer::Gpu(Box::new(VelloWindowRenderer::new()))
    };

    let app = App {
        editor: common::open(path.as_deref(), wrap),
        text_cx: TextContext::new(),
        renderer,
        window: None,
        modifiers: ModifiersState::default(),
        pointer_pos: Point::ZERO,
        last_click: None,
        pending_paste: None,
        ime_enabled: false,
        timer_deadline: None,
    };

    let event_loop = EventLoop::new().unwrap();
    event_loop.run_app(app).expect("Event loop failed");
}
