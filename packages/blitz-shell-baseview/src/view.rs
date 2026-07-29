use std::sync::Arc;
use std::task::Waker;
use std::time::Instant;

use anyrender::WindowRenderer;
use atomic_refcell::AtomicRefCell;
use baseview::dpi::PhysicalPosition;
use baseview::{Event, EventStatus, MouseEvent, WindowContext, WindowEvent, WindowSize};
use blitz_dom::Document;
use blitz_paint::paint_scene;
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta, BlitzWheelEvent, MouseEventButtons,
    PointerCoords, PointerDetails, UiEvent,
};
use blitz_traits::shell::Viewport;
use keyboard_types::Modifiers;

use crate::convert_events::{
    baseview_key_event_to_blitz, baseview_mouse_button_to_blitz, cursor_icon_to_baseview,
    kbt08_modifiers_to_kbt07,
};
use crate::shell_provider::{BaseviewShellProvider, SharedShellState, create_waker};

/// A Blitz document rendering into a baseview window.
pub(crate) struct View<Rend: WindowRenderer> {
    pub(crate) doc: Box<dyn Document>,
    pub(crate) renderer: Rend,
    window: WindowContext,
    shared: Arc<SharedShellState>,
    waker: Waker,

    /// Keyboard modifier state. baseview delivers modifiers with every mouse and
    /// keyboard event, so this is simply the most recently seen state.
    keyboard_modifiers: Modifiers,
    buttons: MouseEventButtons,
    pointer_pos: PhysicalPosition<f64>,
    /// Always empty (baseview is mouse-only), but shared into every dispatched
    /// pointer event to satisfy the `BlitzPointerEvent` API.
    active_events: Arc<AtomicRefCell<Vec<BlitzPointerEvent>>>,
    animation_timer: Option<Instant>,
}

impl<Rend: WindowRenderer> View<Rend> {
    pub(crate) fn init(
        mut doc: Box<dyn Document>,
        mut renderer: Rend,
        window: WindowContext,
    ) -> Self {
        let shared = Arc::new(SharedShellState::default());
        let waker = create_waker(&shared);

        let size = window.size();
        let scale = size.scale_factor;
        let (width, height) = (size.physical.width, size.physical.height);

        {
            let mut inner = doc.inner_mut();
            // Preserve any color-scheme the embedder set on the document
            // (baseview has no system theme detection).
            let color_scheme = inner.viewport().color_scheme;
            inner.set_viewport(Viewport::new(width, height, scale as f32, color_scheme));
            inner.set_shell_provider(Arc::new(BaseviewShellProvider(Arc::clone(&shared))));
        }

        // Resume the renderer. On native platforms (baseview does not support wasm)
        // renderer initialization completes synchronously inside `resume`, so
        // `complete_resume` can be called immediately.
        renderer.resume(Arc::new(window.platform_handle()), width, height, || {});
        let resumed = renderer.complete_resume();
        debug_assert!(resumed, "baseview renderers must resume synchronously");

        // Paint the first frame on the first `on_frame` callback
        shared.request_redraw();

        Self {
            doc,
            renderer,
            window,
            shared,
            waker,
            keyboard_modifiers: Modifiers::default(),
            buttons: MouseEventButtons::None,
            pointer_pos: PhysicalPosition::default(),
            active_events: Arc::new(AtomicRefCell::new(Vec::new())),
            animation_timer: None,
        }
    }

    fn current_animation_time(&mut self) -> f64 {
        match &self.animation_timer {
            Some(start) => Instant::now().duration_since(*start).as_secs_f64(),
            None => {
                self.animation_timer = Some(Instant::now());
                0.0
            }
        }
    }

    pub(crate) fn poll(&mut self) -> bool {
        let waker = self.waker.clone();
        let cx = std::task::Context::from_waker(&waker);
        if self.doc.poll(Some(cx)) {
            self.shared.request_redraw();
            return true;
        }
        false
    }

    fn redraw(&mut self) {
        let animation_time = self.current_animation_time();

        let mut inner = self.doc.inner_mut();
        inner.resolve(animation_time);

        let (width, height) = inner.viewport().window_size;
        let scale = inner.viewport().scale_f64();
        let is_animating = inner.is_animating();
        let is_blocked = inner.has_pending_critical_resources();

        if !is_blocked && width > 0 && height > 0 {
            self.renderer
                .render(|scene| paint_scene(scene, &mut inner, scale, width, height, 0, 0));
        }

        drop(inner);

        if !is_blocked && is_animating {
            self.shared.request_redraw();
        }
    }

    pub(crate) fn set_size(&mut self, new_size: WindowSize) {
        let (width, height) = (new_size.physical.width, new_size.physical.height);
        {
            let mut inner = self.doc.inner_mut();
            let mut viewport = inner.viewport_mut();
            viewport.window_size = (width, height);
            viewport.set_hidpi_scale(new_size.scale_factor as f32);
        }
        if width > 0 && height > 0 {
            self.renderer.set_size(width, height);
            self.shared.request_redraw();
        }
    }

    fn pointer_coords(&self) -> PointerCoords {
        let inner = self.doc.inner();
        let scale = inner.viewport().scale_f64();
        let logical = self.pointer_pos.to_logical::<f64>(scale);
        let (client_x, client_y) = (logical.x as f32, logical.y as f32);
        let viewport_scroll = inner.viewport_scroll();

        PointerCoords {
            screen_x: client_x,
            screen_y: client_y,
            client_x,
            client_y,
            page_x: client_x + viewport_scroll.x as f32,
            page_y: client_y + viewport_scroll.y as f32,
        }
    }

    fn pointer_event(&self, button: blitz_traits::events::MouseEventButton) -> BlitzPointerEvent {
        BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords: self.pointer_coords(),
            button,
            buttons: self.buttons,
            mods: self.keyboard_modifiers,
            details: PointerDetails::default(),
            element: Default::default(),
            active_pointers: Arc::clone(&self.active_events),
        }
    }

    fn handle_mouse_event(&mut self, event: MouseEvent) {
        match event {
            MouseEvent::CursorMoved {
                position,
                modifiers,
            } => {
                self.keyboard_modifiers = kbt08_modifiers_to_kbt07(modifiers);
                self.pointer_pos = position;
                let event = self.pointer_event(Default::default());
                self.doc.handle_ui_event(UiEvent::PointerMove(event));
            }
            MouseEvent::ButtonPressed { button, modifiers } => {
                self.keyboard_modifiers = kbt08_modifiers_to_kbt07(modifiers);
                let button = baseview_mouse_button_to_blitz(button);
                self.buttons |= button.into();
                let event = self.pointer_event(button);
                self.doc.handle_ui_event(UiEvent::PointerDown(event));
                self.shared.request_redraw();
            }
            MouseEvent::ButtonReleased { button, modifiers } => {
                self.keyboard_modifiers = kbt08_modifiers_to_kbt07(modifiers);
                let button = baseview_mouse_button_to_blitz(button);
                self.buttons ^= button.into();
                let event = self.pointer_event(button);
                self.doc.handle_ui_event(UiEvent::PointerUp(event));
                self.shared.request_redraw();
            }
            MouseEvent::WheelScrolled { delta, modifiers } => {
                self.keyboard_modifiers = kbt08_modifiers_to_kbt07(modifiers);
                let delta = match delta {
                    baseview::ScrollDelta::Lines { x, y } => {
                        BlitzWheelDelta::Lines(x as f64, y as f64)
                    }
                    baseview::ScrollDelta::Pixels { x, y } => {
                        BlitzWheelDelta::Pixels(x as f64, y as f64)
                    }
                };
                let event = BlitzWheelEvent {
                    delta,
                    coords: self.pointer_coords(),
                    buttons: self.buttons,
                    mods: self.keyboard_modifiers,
                    element: Default::default(),
                };
                self.doc.handle_ui_event(UiEvent::Wheel(event));
            }
            // baseview only reports these on some platforms, and Blitz infers
            // enter/leave from pointer moves.
            MouseEvent::CursorEntered | MouseEvent::CursorLeft => {}
            // TODO: Drag-and-drop events
            _ => {}
        }
    }

    fn handle_keyboard_event(&mut self, event: &keyboard_types_08::KeyboardEvent) {
        self.keyboard_modifiers = kbt08_modifiers_to_kbt07(event.modifiers);
        let key_event_data = baseview_key_event_to_blitz(event);

        // Ctrl/Super zoom shortcuts (matching blitz-shell behaviour)
        if key_event_data.state.is_pressed()
            && key_event_data
                .modifiers
                .intersects(Modifiers::CONTROL | Modifiers::META)
        {
            use keyboard_types::Code;
            match key_event_data.code {
                Code::Equal => self.doc.inner_mut().viewport_mut().zoom_by(0.1),
                Code::Minus => self.doc.inner_mut().viewport_mut().zoom_by(-0.1),
                Code::Digit0 => self.doc.inner_mut().viewport_mut().set_zoom(1.0),
                _ => {}
            }
        }

        let event = if key_event_data.state.is_pressed() {
            UiEvent::KeyDown(key_event_data)
        } else {
            UiEvent::KeyUp(key_event_data)
        };
        self.doc.handle_ui_event(event);
    }

    pub(crate) fn handle_event(&mut self, event: Event) -> EventStatus {
        let status = match &event {
            // Blitz always consumes keyboard events. A smarter implementation might
            // return `Ignored` when no input element is focused, so that (e.g.) a DAW
            // host can use the keyboard for playing notes.
            Event::Keyboard(_) => EventStatus::Captured,
            Event::Mouse(_) => EventStatus::Captured,
            Event::Window(_) => EventStatus::Ignored,
            _ => EventStatus::Ignored,
        };

        match event {
            Event::Mouse(mouse_event) => self.handle_mouse_event(mouse_event),
            Event::Keyboard(keyboard_event) => self.handle_keyboard_event(&keyboard_event),
            Event::Window(window_event) => match window_event {
                WindowEvent::WillClose => self.renderer.suspend(),
                WindowEvent::Focused | WindowEvent::Unfocused => {}
                _ => {}
            },
            _ => {}
        }

        // Poll the document so that side-effects of the event are processed promptly
        self.poll();

        status
    }

    pub(crate) fn on_frame(&mut self) {
        if self.shared.take(&self.shared.poll_requested) {
            self.poll();
        }

        if let Some(cursor) = self.shared.cursor.lock().unwrap().take() {
            self.window
                .set_mouse_cursor(cursor_icon_to_baseview(cursor));
        }

        if self.shared.take(&self.shared.close_requested) {
            self.window.request_close();
        }

        // Render every frame rather than only when a redraw has been requested.
        //
        // Skipping clean frames is not currently safe: renderers can silently skip
        // presenting a frame (e.g. wgpu skips occluded windows on macOS to avoid a
        // `nextDrawable` hang) with no feedback, so a consumed redraw request could
        // be dropped and leave the window permanently stale. Style/layout resolution
        // is incremental, and a skipped present is cheap, so the overhead of
        // rendering unconditionally is acceptable (and is what other baseview GUI
        // integrations do).
        let _ = self.shared.take(&self.shared.redraw_requested);
        if self.renderer.is_active() {
            self.redraw();
        }
    }
}
