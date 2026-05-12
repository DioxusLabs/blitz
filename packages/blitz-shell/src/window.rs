use crate::BlitzShellProvider;
use crate::convert_events::{
    button_source_to_blitz, color_scheme_to_theme, pointer_source_to_blitz,
    pointer_source_to_blitz_details, theme_to_color_scheme, winit_ime_to_blitz,
    winit_key_event_to_blitz, winit_modifiers_to_kbt_modifiers,
};
use crate::event::{BlitzShellEvent, BlitzShellProxy, create_waker};
use anyrender::WindowRenderer;
use blitz_dom::Document;
use blitz_paint::paint_scene;
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta, BlitzWheelEvent, MouseEventButton,
    MouseEventButtons, PointerCoords, PointerDetails, UiEvent,
};
use blitz_traits::shell::Viewport;
use winit::dpi::{LogicalPosition, PhysicalInsets, PhysicalPosition};
use winit::keyboard::PhysicalKey;

use std::any::Any;
use std::sync::Arc;
use std::task::Waker;
use web_time::Instant;
use winit::event::{ButtonSource, ElementState, MouseButton};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Theme, WindowAttributes, WindowId};
use winit::{event::Modifiers, event::WindowEvent, keyboard::KeyCode, window::Window};

#[cfg(feature = "accessibility")]
use crate::accessibility::AccessibilityState;

// Ignore safe_area_insets on macOS because we don't want to avoid
// drawing in the titlebar.
#[cfg(target_os = "macos")]
fn get_safe_area_insets(_window: &dyn Window) -> PhysicalInsets<u32> {
    Default::default()
}
#[cfg(not(target_os = "macos"))]
fn get_safe_area_insets(window: &dyn Window) -> PhysicalInsets<u32> {
    window.safe_area()
}

pub struct WindowConfig<Rend: WindowRenderer> {
    doc: Box<dyn Document>,
    pub(crate) attributes: WindowAttributes,
    renderer: Rend,
}

impl<Rend: WindowRenderer> WindowConfig<Rend> {
    pub fn new(doc: Box<dyn Document>, renderer: Rend) -> Self {
        Self::with_attributes(doc, renderer, WindowAttributes::default())
    }

    pub fn with_attributes(
        doc: Box<dyn Document>,
        renderer: Rend,
        attributes: WindowAttributes,
    ) -> Self {
        WindowConfig {
            doc,
            attributes,
            renderer,
        }
    }
}

pub struct View<Rend: WindowRenderer> {
    pub doc: Box<dyn Document>,

    pub renderer: Rend,
    pub waker: Option<Waker>,

    pub proxy: BlitzShellProxy,
    pub window: Arc<dyn Window>,

    /// The state of the keyboard modifiers (ctrl, shift, etc). Winit/Tao don't track these for us so we
    /// need to store them in order to have access to them when processing keypress events
    pub theme_override: Option<Theme>,
    pub keyboard_modifiers: Modifiers,
    pub buttons: MouseEventButtons,
    pub pointer_pos: PhysicalPosition<f64>,
    pub animation_timer: Option<Instant>,
    pub is_visible: bool,
    pub safe_area_insets: PhysicalInsets<u32>,

    #[cfg(target_arch = "wasm32")]
    pending_resize: Option<winit::dpi::PhysicalSize<u32>>,
    #[cfg(target_arch = "wasm32")]
    last_resize_at: Option<web_time::Instant>,
    /// True iff a setTimeout has been scheduled and not yet observed by
    /// `apply_pending_resize_if_settled`. Prevents the timer storm that would
    /// otherwise allocate a fresh `Closure` per resize event during a drag.
    #[cfg(target_arch = "wasm32")]
    resize_timer_scheduled: bool,

    #[cfg(feature = "accessibility")]
    /// Accessibility adapter for `accesskit`.
    pub accessibility: AccessibilityState,

    // Calling request_redraw within a WindowEvent doesn't work on iOS. So on iOS we track the state
    // with a boolean and call request_redraw in about_to_wait
    //
    // See https://github.com/rust-windowing/winit/issues/3406
    #[cfg(target_os = "ios")]
    pub ios_request_redraw: std::cell::Cell<bool>,
}

impl<Rend: WindowRenderer> View<Rend> {
    pub fn init(
        config: WindowConfig<Rend>,
        event_loop: &dyn ActiveEventLoop,
        proxy: &BlitzShellProxy,
    ) -> Self {
        // We create window as invisble and then later make window visible
        // after AccessKit has initialised to avoid AccessKit panics
        let is_visible = config.attributes.visible;
        // Capture the requested surface size before consuming `attributes`, so we can
        // seed the viewport on platforms (winit-web) that report `surface_size() == 0×0`
        // until a layout pass fires.
        let requested_surface_size = config.attributes.surface_size;
        let attrs = config.attributes.with_visible(false);

        let winit_window: Arc<dyn Window> = Arc::from(event_loop.create_window(attrs).unwrap());
        #[cfg(feature = "accessibility")]
        let accessibility = AccessibilityState::new(&*winit_window, proxy.clone());

        if is_visible {
            winit_window.set_visible(true);
        }

        // Create viewport
        // TODO: account for the "safe area"
        let scale = winit_window.scale_factor() as f32;
        let mut size = winit_window.surface_size();
        if (size.width == 0 || size.height == 0)
            && let Some(requested) = requested_surface_size
        {
            size = requested.to_physical(scale as f64);
        }
        // On wasm, when the embedder didn't call `with_surface_size`, winit-web's
        // initial `surface_size()` is 0×0 — its ResizeObserver hasn't fired yet.
        // Resuming the renderer at 0×0 trips a wgpu swapchain-size-0 error, so
        // seed from the canvas element's CSS layout box (host-stylesheet result).
        #[cfg(target_arch = "wasm32")]
        if size.width == 0 || size.height == 0 {
            use winit::platform::web::WindowExtWeb;
            if let Some(canvas) = winit_window.canvas() {
                let css_w = canvas.offset_width().max(0) as u32;
                let css_h = canvas.offset_height().max(0) as u32;
                if css_w > 0 && css_h > 0 {
                    size = winit::dpi::LogicalSize::new(css_w, css_h).to_physical(scale as f64);
                }
            }
        }
        let safe_area_insets = get_safe_area_insets(&*winit_window);
        let theme = winit_window.theme().unwrap_or(Theme::Light);
        let color_scheme = theme_to_color_scheme(theme);
        let viewport = Viewport::new(size.width, size.height, scale, color_scheme);

        // Create shell provider
        let shell_provider = BlitzShellProvider::new(winit_window.clone());

        let mut doc = config.doc;
        let mut inner = doc.inner_mut();
        inner.set_viewport(viewport);
        inner.set_shell_provider(Arc::new(shell_provider));

        // If the document title is set prior to the window being created then it will
        // have been sent to a dummy ShellProvider and won't get picked up.
        // So we look for it here and set it if present.
        let title = inner.find_title_node().map(|node| node.text_content());
        if let Some(title) = title {
            winit_window.set_title(&title);
        }

        drop(inner);

        Self {
            renderer: config.renderer,
            waker: None,
            animation_timer: None,
            keyboard_modifiers: Default::default(),
            proxy: proxy.clone(),
            window: winit_window.clone(),
            doc,
            theme_override: None,
            buttons: MouseEventButtons::None,
            safe_area_insets,
            #[cfg(target_arch = "wasm32")]
            pending_resize: None,
            #[cfg(target_arch = "wasm32")]
            last_resize_at: None,
            #[cfg(target_arch = "wasm32")]
            resize_timer_scheduled: false,
            pointer_pos: Default::default(),
            is_visible: winit_window.is_visible().unwrap_or(true),
            #[cfg(feature = "accessibility")]
            accessibility,

            #[cfg(target_os = "ios")]
            ios_request_redraw: std::cell::Cell::new(false),
        }
    }

    pub fn replace_document(&mut self, new_doc: Box<dyn Document>, retain_scroll_position: bool) {
        let inner = self.doc.inner();
        let scroll = inner.viewport_scroll();
        let viewport = inner.viewport().clone();
        let shell_provider = inner.shell_provider.clone();
        drop(inner);

        self.doc = new_doc;

        let mut inner = self.doc.inner_mut();
        inner.set_viewport(viewport);
        inner.set_shell_provider(shell_provider);
        drop(inner);

        self.poll();
        self.request_redraw();

        if retain_scroll_position {
            self.doc.inner_mut().set_viewport_scroll(scroll);
        }
    }

    pub fn theme_override(&self) -> Option<Theme> {
        self.theme_override
    }

    pub fn current_theme(&self) -> Theme {
        color_scheme_to_theme(self.doc.inner().viewport().color_scheme)
    }

    pub fn set_theme_override(&mut self, theme: Option<Theme>) {
        self.theme_override = theme;
        let theme = theme.or(self.window.theme()).unwrap_or(Theme::Light);
        self.with_viewport(|v| v.color_scheme = theme_to_color_scheme(theme));
    }

    pub fn downcast_doc_mut<T: 'static>(&mut self) -> &mut T {
        (&mut *self.doc as &mut dyn Any)
            .downcast_mut::<T>()
            .unwrap()
    }

    pub fn current_animation_time(&mut self) -> f64 {
        match &self.animation_timer {
            Some(start) => Instant::now().duration_since(*start).as_secs_f64(),
            None => {
                self.animation_timer = Some(Instant::now());
                0.0
            }
        }
    }
}

impl<Rend: WindowRenderer> View<Rend> {
    /// Start resuming the renderer. Dispatches [`BlitzShellEvent::ResumeReady`]
    /// when initialization completes — synchronously on native, asynchronously
    /// on wasm32. The embedder must call [`complete_resume`](Self::complete_resume)
    /// in response.
    pub fn resume(&mut self) {
        let window_id = self.window_id();
        let animation_time = self.current_animation_time();

        let (width, height) = {
            let mut inner = self.doc.inner_mut();
            inner.resolve(animation_time);
            inner.viewport().window_size
        };

        let proxy = self.proxy.clone();
        self.renderer
            .resume(Arc::new(self.window.clone()), width, height, move || {
                proxy.send_event(BlitzShellEvent::ResumeReady { window_id });
            });
    }

    /// Finalize a previously-started resume. Should be called in response to a
    /// [`BlitzShellEvent::ResumeReady`] event. Paints the first frame and
    /// installs the doc poll waker. Returns `true` if the renderer is now active.
    pub fn complete_resume(&mut self) -> bool {
        if !self.renderer.complete_resume() {
            return false;
        }

        let window_id = self.window_id();

        // Resync the renderer to the current viewport. Resize/scale events that
        // arrived while the renderer was Pending were no-ops on the renderer
        // (its `set_size` only matches Active), so the surface created during
        // resume could be at a stale size by the time we get here.
        let animation_time = self.current_animation_time();
        let mut inner = self.doc.inner_mut();
        inner.resolve(animation_time);
        let (width, height) = inner.viewport().window_size;
        let scale = inner.viewport().scale_f64();
        let insets = self.safe_area_insets.to_logical(scale);

        #[cfg(feature = "custom-widget")]
        inner.can_create_surfaces(&self.renderer as _);

        self.renderer.set_size(width, height);

        self.renderer.render(|scene| {
            paint_scene(
                scene,
                &mut inner,
                scale,
                width,
                height,
                insets.left,
                insets.top,
            )
        });

        self.waker = Some(create_waker(&self.proxy, window_id));
        true
    }

    pub fn suspend(&mut self) {
        self.waker = None;
        self.renderer.suspend();

        #[cfg(feature = "custom-widget")]
        self.doc.inner_mut().destroy_surfaces();
    }

    pub fn poll(&mut self) -> bool {
        if let Some(waker) = &self.waker {
            let cx = std::task::Context::from_waker(waker);
            if self.doc.poll(Some(cx)) {
                #[cfg(feature = "accessibility")]
                {
                    let inner = self.doc.inner();
                    if inner.has_changes() {
                        self.accessibility.update_tree(&inner);
                    }
                }

                self.request_redraw();
                return true;
            }
        }

        false
    }

    pub fn request_redraw(&self) {
        if self.renderer.is_active() {
            self.window.request_redraw();
            #[cfg(target_os = "ios")]
            self.ios_request_redraw.set(true);
        }
    }

    pub fn redraw(&mut self) {
        #[cfg(target_os = "ios")]
        self.ios_request_redraw.set(false);
        let animation_time = self.current_animation_time();
        let is_visible = self.is_visible;

        let mut inner = self.doc.inner_mut();
        inner.resolve(animation_time);

        // Unregister resources (e.g. textures) from dropped custom widget nodes
        #[cfg(feature = "custom-widget")]
        for id in inner.take_pending_resource_deallocations() {
            self.renderer.unregister_resource(id);
        }

        let (width, height) = inner.viewport().window_size;
        let scale = inner.viewport().scale_f64();
        let is_animating = inner.is_animating();
        let is_blocked = inner.has_pending_critical_resources();
        let insets = self.safe_area_insets.to_logical(scale);

        if !is_blocked && is_visible {
            self.renderer.render(|scene| {
                paint_scene(
                    scene,
                    &mut inner,
                    scale,
                    width,
                    height,
                    insets.left,
                    insets.top,
                )
            });
        }

        drop(inner);

        if !is_blocked && is_visible && is_animating {
            self.request_redraw();
        }
    }

    pub fn pointer_coords(&self, position: PhysicalPosition<f64>) -> PointerCoords {
        let inner = self.doc.inner();
        let scale = inner.viewport().scale_f64();
        let LogicalPosition::<f32> {
            x: screen_x,
            y: screen_y,
        } = position.to_logical(scale);
        let viewport_scroll_offset = inner.viewport_scroll();
        let client_x = screen_x - (self.safe_area_insets.left as f64 / scale) as f32;
        let client_y = screen_y - (self.safe_area_insets.top as f64 / scale) as f32;
        let page_x = client_x + viewport_scroll_offset.x as f32;
        let page_y = client_y + viewport_scroll_offset.y as f32;

        PointerCoords {
            screen_x,
            screen_y,
            client_x,
            client_y,
            page_x,
            page_y,
        }
    }

    pub fn window_id(&self) -> WindowId {
        self.window.id()
    }

    #[inline]
    pub fn with_viewport(&mut self, cb: impl FnOnce(&mut Viewport)) {
        let mut inner = self.doc.inner_mut();
        let mut viewport = inner.viewport_mut();
        cb(&mut viewport);
        let (width, height) = viewport.window_size;
        drop(viewport);
        drop(inner);
        if width > 0 && height > 0 {
            let insets = self.safe_area_insets;
            self.renderer.set_size(
                width + insets.left + insets.right,
                height + insets.top + insets.bottom,
            );
            self.request_redraw();
        }
    }

    #[cfg(feature = "accessibility")]
    pub fn build_accessibility_tree(&mut self) {
        let inner = self.doc.inner();
        self.accessibility.update_tree(&inner);
    }

    #[cfg(target_arch = "wasm32")]
    const RESIZE_DEBOUNCE_MS: u32 = 100;

    #[cfg(target_arch = "wasm32")]
    fn schedule_resize_settle_check(&mut self, delay_ms: u32) {
        use wasm_bindgen::JsCast;
        use wasm_bindgen::closure::Closure;

        let proxy = self.proxy.clone();
        let window_id = self.window_id();
        let cb = Closure::once_into_js(move || {
            proxy.send_event(BlitzShellEvent::ResizeSettleCheck { window_id });
        });
        if let Some(win) = web_sys::window() {
            let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(
                cb.unchecked_ref(),
                delay_ms as i32,
            );
            self.resize_timer_scheduled = true;
        }
    }

    /// Applies the pending resize iff motion has been quiet for the debounce
    /// window; otherwise re-arms the timer for the remaining time. Called
    /// when a previously scheduled timer fires.
    #[cfg(target_arch = "wasm32")]
    pub fn apply_pending_resize_if_settled(&mut self) {
        self.resize_timer_scheduled = false;
        let Some(last) = self.last_resize_at else {
            return;
        };
        let debounce = std::time::Duration::from_millis(Self::RESIZE_DEBOUNCE_MS as u64);
        let elapsed = web_time::Instant::now().saturating_duration_since(last);
        if elapsed < debounce {
            // Motion ongoing — wait out the rest of the window before re-checking.
            let remaining_ms = (debounce - elapsed).as_millis() as u32;
            self.schedule_resize_settle_check(remaining_ms);
            return;
        }
        let Some(size) = self.pending_resize.take() else {
            return;
        };
        self.last_resize_at = None;

        let insets = self.safe_area_insets;
        let width = size.width.saturating_sub(insets.left + insets.right);
        let height = size.height.saturating_sub(insets.top + insets.bottom);
        self.with_viewport(|v| v.window_size = (width, height));
        self.request_redraw();
    }

    #[cfg(target_os = "macos")]
    pub fn handle_apple_standard_keybinding(&mut self, command: &str) {
        use blitz_traits::SmolStr;
        let event = UiEvent::AppleStandardKeybinding(SmolStr::new(command));
        self.doc.handle_ui_event(event);
    }

    pub fn handle_winit_event(&mut self, event: WindowEvent) {
        // Update accessibility focus and window size state in response to a Winit WindowEvent
        #[cfg(feature = "accessibility")]
        self.accessibility
            .process_window_event(&*self.window, &event);

        match event {
            WindowEvent::Destroyed => {}
            WindowEvent::ActivationTokenDone { .. } => {},
            WindowEvent::CloseRequested => {
                // Currently handled at the level above in application.rs
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
            }
            WindowEvent::Moved(_) => {}
            WindowEvent::Occluded(is_occluded) => {
                self.is_visible = !is_occluded;
                if self.is_visible {
                    self.request_redraw();
                }
            },
            WindowEvent::SurfaceResized(physical_size) => {
                self.safe_area_insets = get_safe_area_insets(&*self.window);
                // On WASM, defer the apply: wgpu's surface.configure clears the canvas,
                // so running it every frame flickers during a drag. The browser stretches
                // the stale backing store until the debounce timer settles.
                #[cfg(target_arch = "wasm32")]
                {
                    self.pending_resize = Some(physical_size);
                    self.last_resize_at = Some(web_time::Instant::now());
                    if !self.resize_timer_scheduled {
                        self.schedule_resize_settle_check(Self::RESIZE_DEBOUNCE_MS);
                    }
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let insets = self.safe_area_insets;
                    let width = physical_size.width - insets.left - insets.right;
                    let height = physical_size.height - insets.top - insets.bottom;
                    self.with_viewport(|v| v.window_size = (width, height));
                    self.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.with_viewport(|v| v.set_hidpi_scale(scale_factor as f32));
                self.request_redraw();
            }
            WindowEvent::ThemeChanged(theme) => {
                let color_scheme = theme_to_color_scheme(self.theme_override.unwrap_or(theme));
                let mut inner = self.doc.inner_mut();
                inner.viewport_mut().color_scheme = color_scheme;
            }
            WindowEvent::Ime(ime_event) => {
                self.doc.handle_ui_event(UiEvent::Ime(winit_ime_to_blitz(ime_event)));
                self.request_redraw();
            },
            WindowEvent::ModifiersChanged(new_state) => {
                // Store new keyboard modifier (ctrl, shift, etc) state for later use
                self.keyboard_modifiers = new_state;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(key_code) = event.physical_key && event.state.is_pressed() {
                        let ctrl = self.keyboard_modifiers.state().control_key();
                        let meta = self.keyboard_modifiers.state().meta_key();
                        let alt = self.keyboard_modifiers.state().alt_key();

                        // Ctrl/Super keyboard shortcuts
                        if ctrl | meta {
                            match key_code {
                                KeyCode::Equal => {
                                    self.doc.inner_mut().viewport_mut().zoom_by(0.1);
                                },
                                KeyCode::Minus => {
                                    self.doc.inner_mut().viewport_mut().zoom_by(-0.1);
                                },
                                KeyCode::Digit0 => {
                                    self.doc.inner_mut().viewport_mut().set_zoom(1.0);
                                }
                                _ => {}
                            };
                        }

                        // Alt keyboard shortcuts
                        if alt {
                            match key_code {
                                KeyCode::KeyD => {
                                    let mut inner = self.doc.inner_mut();
                                    inner.devtools_mut().toggle_show_layout();
                                    drop(inner);
                                    self.request_redraw();
                                }
                                KeyCode::KeyH => {
                                    let mut inner = self.doc.inner_mut();
                                    inner.devtools_mut().toggle_highlight_hover();
                                    drop(inner);
                                    self.request_redraw();
                                }
                                KeyCode::KeyT => self.doc.inner().print_taffy_tree(),
                                _ => {}
                            };
                        }

                }

                // Unmodified keypresses
                let key_event_data = winit_key_event_to_blitz(&event, self.keyboard_modifiers.state());
                let event = if event.state.is_pressed() {
                    UiEvent::KeyDown(key_event_data)
                } else {
                    UiEvent::KeyUp(key_event_data)
                };

                self.doc.handle_ui_event(event);
            }
            WindowEvent::PointerEntered { /*device_id*/.. } => {}
            WindowEvent::PointerLeft { /*device_id*/.. } => {}
            WindowEvent::PointerMoved { position, source, primary, .. } => {
                self.pointer_pos = position;
                let event = UiEvent::PointerMove(BlitzPointerEvent {
                    id: pointer_source_to_blitz(&source),
                    is_primary: primary,
                    coords: self.pointer_coords(position),
                    button: Default::default(),
                    buttons: self.buttons,
                    mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                    details: pointer_source_to_blitz_details(&source)
                });
                self.doc.handle_ui_event(event);
            }
            WindowEvent::PointerButton { button, state, primary, position, .. } => {
                let id = button_source_to_blitz(&button);
                let coords = self.pointer_coords(position);
                self.pointer_pos = position;
                let button = match &button {
                    ButtonSource::Mouse(mouse_button) => match mouse_button {
                        MouseButton::Left => MouseEventButton::Main,
                        MouseButton::Right => MouseEventButton::Secondary,
                        MouseButton::Middle => MouseEventButton::Auxiliary,
                        // TODO: handle other button types
                        _ => MouseEventButton::Auxiliary,
                    }
                    _ => MouseEventButton::Main,
                };

                match state {
                    ElementState::Pressed => self.buttons |= button.into(),
                    ElementState::Released => self.buttons ^= button.into(),
                }

                if id != BlitzPointerId::Mouse {
                    let event = UiEvent::PointerMove(BlitzPointerEvent {
                        id,
                        is_primary: primary,
                        coords,
                        button: Default::default(),
                        buttons: self.buttons,
                        mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                        details: PointerDetails::default()
                    });
                    self.doc.handle_ui_event(event);
                }

                let event = BlitzPointerEvent {
                    id,
                    is_primary: primary,
                    coords,
                    button,
                    buttons: self.buttons,
                    mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),

                    // TODO: details for pointer up/down events
                    details: PointerDetails::default(),
                };

                let event = match state {
                    ElementState::Pressed => UiEvent::PointerDown(event),
                    ElementState::Released => UiEvent::PointerUp(event),
                };

                self.doc.handle_ui_event(event);
                self.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let blitz_delta = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => BlitzWheelDelta::Lines(x as f64, y as f64),
                    winit::event::MouseScrollDelta::PixelDelta(pos) => BlitzWheelDelta::Pixels(pos.x, pos.y),
                };

                let event = BlitzWheelEvent {
                    delta: blitz_delta,
                    coords: self.pointer_coords(self.pointer_pos),
                    buttons: self.buttons,
                    mods: winit_modifiers_to_kbt_modifiers(self.keyboard_modifiers.state()),
                };

                self.doc.handle_ui_event(UiEvent::Wheel(event));
            }
            WindowEvent::Focused(_) => {}
            WindowEvent::TouchpadPressure { .. } => {}
            WindowEvent::PinchGesture { .. } => {},
            WindowEvent::PanGesture { .. } => {},
            WindowEvent::DoubleTapGesture { .. } => {},
            WindowEvent::RotationGesture { .. } => {},
            WindowEvent::DragEntered { .. } => {},
            WindowEvent::DragMoved { .. } => {},
            WindowEvent::DragDropped { .. } => {},
            WindowEvent::DragLeft { .. } => {},
        }
    }
}
