//! Render HTML/CSS to RGBA pixel buffers with Blitz, for embedding in native
//! home-screen/lock-screen widgets (iOS WidgetKit, Android AppWidget).
//!
//! Widgets on both platforms cannot host a live GPU surface or event loop, so
//! this renders a Blitz document to a bitmap on the CPU which the native
//! widget shell displays as an image.
//!
//! Interactivity: widgets only support discrete, declarative tap targets
//! (`Button(intent:)` on iOS, `PendingIntent` on Android). To make arbitrary
//! HTML elements tappable, elements carry a `data-action` attribute; after
//! layout resolution their absolute rects are extracted from the Blitz DOM so
//! the native shell can overlay an invisible tap target per element and map
//! taps back to actions.

pub mod demo;
pub mod store;

use anyrender::{PaintScene as _, render_to_buffer};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{BaseDocument, DocumentConfig, NodeId, util::Color};
use blitz_html::HtmlDocument;
use blitz_paint::color::ToColorColor as _;
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use peniko::Fill;
use peniko::kurbo::Rect;

/// A tappable region extracted from the Blitz DOM: the layout rect (in CSS
/// px / points, relative to the document origin) of an element that carries a
/// `data-action` attribute.
#[derive(Debug, Clone)]
pub struct HitRegion {
    pub action: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Render an HTML string to an RGBA8888 pixel buffer of `width * scale` x
/// `height * scale` physical pixels, and extract the hit regions of all
/// elements with a `data-action` attribute.
///
/// `time` is the CSS animation clock in seconds: the document is first
/// resolved at t=0 (starting all CSS animations/transitions), then re-resolved
/// at `time`, so the output samples every animation at exactly that instant.
pub fn render_html(
    html: &str,
    width: u32,
    height: u32,
    scale: f64,
    time: f64,
) -> (Vec<u8>, Vec<HitRegion>) {
    let render_width = (width as f64 * scale) as u32;
    let render_height = (height as f64 * scale) as u32;

    let mut document = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(
                render_width,
                render_height,
                scale as f32,
                ColorScheme::Light,
            )),
            ..Default::default()
        },
    );

    document.as_mut().resolve(0.0);
    if time > 0.0 {
        document.as_mut().resolve(time);
    }

    let regions = extract_hit_regions(document.as_ref());
    let buffer = paint_document(&mut document, render_width, render_height, scale);
    (buffer, regions)
}

fn paint_document(
    document: &mut HtmlDocument,
    render_width: u32,
    render_height: u32,
    scale: f64,
) -> Vec<u8> {
    render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| {
            scene.fill(
                Fill::NonZero,
                Default::default(),
                Color::TRANSPARENT,
                Default::default(),
                &Rect::new(0.0, 0.0, render_width as f64, render_height as f64),
            );
            paint_scene(
                scene,
                document.as_mut(),
                scale,
                render_width,
                render_height,
                0,
                0,
            );
        },
        render_width,
        render_height,
    )
}

/// Build the animated widget's document mid-transition: parse the template
/// with the transitioned elements' inline styles at the `from` pose, resolve
/// it, mutate their inline styles to the `to` pose (which starts CSS
/// transitions in stylo), then resolve at `elapsed` seconds — sampling every
/// transition exactly `elapsed` into its run.
#[allow(clippy::too_many_arguments)]
fn resolve_anim_document(
    from: &demo::Pose,
    to: &demo::Pose,
    elapsed: f64,
    scrub: i32,
    hide_tracked: bool,
    render_width: u32,
    render_height: u32,
    scale: f64,
) -> HtmlDocument {
    let html = demo::animated_html(from, scrub, hide_tracked);
    let mut document = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(
                render_width,
                render_height,
                scale as f32,
                ColorScheme::Light,
            )),
            ..Default::default()
        },
    );
    let doc = document.as_mut();
    doc.resolve(0.0);
    let targets = collect_anim_nodes(doc);
    {
        let mut mutator = doc.mutate();
        for (name, style) in to.styles() {
            if let Some(&node_id) = targets.iter().find(|(n, _)| *n == name).map(|(_, id)| id) {
                mutator.set_attribute(node_id, blitz_dom::qual_name!("style"), &style);
            }
        }
    }
    doc.resolve(0.0);
    if elapsed > 0.0 {
        doc.resolve(elapsed);
    }
    document
}

/// The node of each `data-anim` (transitioned) element.
fn collect_anim_nodes(doc: &BaseDocument) -> Vec<(String, NodeId)> {
    fn walk(doc: &BaseDocument, node_id: NodeId, out: &mut Vec<(String, NodeId)>) {
        let Some(node) = doc.get_node(node_id) else {
            return;
        };
        if let Some(attrs) = node.attrs()
            && let Some(name) = attrs
                .iter()
                .find(|a| a.name.local.as_ref() == "data-anim")
                .map(|a| a.value.to_string())
        {
            out.push((name, node_id));
        }
        for child in node.children.iter() {
            walk(doc, *child, out);
        }
    }
    let mut out = Vec::new();
    walk(doc, doc.root_element().id, &mut out);
    out
}

/// Read the transitioned elements' current pose back out of a resolved
/// document: the ball's rect (relative to its stage), the fill's width as a
/// percentage of its rail, and the badge's computed background color. This is
/// how an interrupted transition is re-baselined — the mid-flight values
/// stylo interpolated become the `from` pose of the next transition, exactly
/// like live CSS.
fn read_pose(doc: &BaseDocument) -> Option<demo::Pose> {
    let targets = collect_anim_nodes(doc);
    let find = |name: &str| {
        targets
            .iter()
            .find(|(n, _)| n == name)
            .and_then(|(_, id)| doc.get_node(*id))
    };

    let ball = find("ball")?;
    let stage = doc.get_node(ball.parent?)?;
    let ball_pos = ball.absolute_position(0.0, 0.0);
    let stage_pos = stage.absolute_position(0.0, 0.0);

    let fill = find("fill")?;
    let rail = doc.get_node(fill.parent?)?;
    let rail_width = rail.final_layout().size.width as f64;
    let fill_pct = if rail_width > 0.0 {
        (fill.final_layout().size.width as f64 / rail_width * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };

    let badge = find("badge")?;
    let styles = badge.primary_styles()?;
    let current_color = styles.clone_color();
    let bg = styles
        .get_background()
        .background_color
        .resolve_to_absolute(&current_color)
        .as_srgb_color();

    Some(demo::Pose {
        ball_left: (ball_pos.x - stage_pos.x) as f64,
        ball_top: (ball_pos.y - stage_pos.y) as f64,
        ball_size: ball.final_layout().size.width as f64,
        fill_pct,
        badge: [
            (bg.components[0] as f64 * 255.0).clamp(0.0, 255.0),
            (bg.components[1] as f64 * 255.0).clamp(0.0, 255.0),
            (bg.components[2] as f64 * 255.0).clamp(0.0, 255.0),
        ],
    })
}

/// Nominal layout size used when a document is resolved only to read a pose
/// back (not to paint pixels). The pose's values are size-independent (the
/// ball's inline px, the fill's percentage), so any reasonable size works.
const POSE_PROBE_SIZE: (u32, u32) = (360, 170);

/// The animated widget's pose at wall-clock `now`: the target pose if no
/// transition is in flight, otherwise the mid-transition pose stylo
/// interpolated — used to re-baseline the next transition when an action
/// interrupts the current one.
pub(crate) fn current_anim_pose(state: &store::WidgetState, now: f64) -> demo::Pose {
    let to = store::target_pose(state, now);
    let elapsed = now - state.trans_start;
    if !store::playing(state, now) && elapsed >= demo::TRANSITION_SECS {
        return to;
    }
    let from = store::from_pose(state);
    let doc = resolve_anim_document(
        &from,
        &to,
        elapsed.clamp(0.0, demo::TRANSITION_SECS + 1.0),
        store::scrub_segment(state),
        false,
        POSE_PROBE_SIZE.0,
        POSE_PROBE_SIZE.1,
        1.0,
    );
    read_pose(doc.as_ref()).unwrap_or(to)
}

/// Render an HTML string to an RGBA8888 pixel buffer (no hit regions).
pub fn render_html_to_rgba(html: &str, width: u32, height: u32, scale: f64) -> Vec<u8> {
    render_html(html, width, height, scale, 0.0).0
}

/// Walk the resolved document and collect the absolute layout rects of all
/// elements carrying a `data-action` attribute, plus all elements carrying a
/// `data-track` attribute (reported with a `track:` action prefix) so native
/// shells can composite tracked elements as separately positioned layers.
pub fn extract_hit_regions(doc: &BaseDocument) -> Vec<HitRegion> {
    let mut regions = Vec::new();
    collect_regions(doc, doc.root_element().id, &mut regions);
    regions
}

fn collect_regions(doc: &BaseDocument, node_id: NodeId, out: &mut Vec<HitRegion>) {
    let Some(node) = doc.get_node(node_id) else {
        return;
    };
    if let Some(attrs) = node.attrs() {
        let action = attrs.iter().find_map(|a| match a.name.local.as_ref() {
            "data-action" => Some(a.value.to_string()),
            "data-track" => Some(format!("track:{}", a.value)),
            _ => None,
        });
        if let Some(action) = action {
            let pos = node.absolute_position(0.0, 0.0);
            let size = node.final_layout().size;
            out.push(HitRegion {
                action,
                x: pos.x as f64,
                y: pos.y as f64,
                width: size.width as f64,
                height: size.height as f64,
            });
        }
    }
    for child in node.children.iter() {
        collect_regions(doc, *child, out);
    }
}

fn regions_to_json(regions: &[HitRegion]) -> String {
    let mut json = String::from("[");
    for (i, r) in regions.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        let action = r.action.replace('\\', "\\\\").replace('"', "\\\"");
        json.push_str(&format!(
            "{{\"action\":\"{}\",\"x\":{:.2},\"y\":{:.2},\"width\":{:.2},\"height\":{:.2}}}",
            action, r.x, r.y, r.width, r.height
        ));
    }
    json.push(']');
    json
}

/// Render a complete widget frame from the Rust-owned state: the background
/// pixels plus a JSON plan of everything the native shell must composite —
/// hit-region buttons (`data-action` rects) and sprite layers (for `anim`
/// with `hide_tracked`, planned from the resolved `data-track` rects). The
/// shell just blits the frame, draws the layers where the plan says, and
/// forwards button taps back as actions.
///
/// Plan shape:
/// `{"buttons":[{"action":..,"x":..,"y":..,"width":..,"height":..},..],
///   "layers":[{"track":..,"x":..,"y":..,"width":..,"height":..,
///              "spriteWidth":..,"spriteHeight":..,"clipWidth":..},..]}`
#[allow(clippy::too_many_arguments)]
pub fn render_widget_frame(
    path: &str,
    kind: &str,
    width: u32,
    height: u32,
    scale: f64,
    time: f64,
    hide_tracked: bool,
    clock: &str,
) -> Option<(Vec<u8>, String)> {
    let (buffer, regions) = if kind == "anim" {
        // `time` is the display offset: seconds from now at which this frame
        // will be shown (0 = immediately). Rust maps it onto the persisted
        // transition/playback clocks, so pre-rendered timeline frames sample
        // the transitions at exactly the instant they will appear.
        let state = store::load(path);
        let display = store::now_epoch() + time.max(0.0);
        let from = store::from_pose(&state);
        let to = store::target_pose(&state, display);
        let elapsed = (display - state.trans_start).clamp(0.0, demo::TRANSITION_SECS + 1.0);
        let render_width = (width as f64 * scale) as u32;
        let render_height = (height as f64 * scale) as u32;
        let mut document = resolve_anim_document(
            &from,
            &to,
            elapsed,
            store::scrub_segment(&state),
            hide_tracked,
            render_width,
            render_height,
            scale,
        );
        let regions = extract_hit_regions(document.as_ref());
        let buffer = paint_document(&mut document, render_width, render_height, scale);
        (buffer, regions)
    } else {
        let html = store::widget_html(path, kind, hide_tracked, clock)?;
        render_html(&html, width, height, scale, time)
    };
    let buttons: Vec<HitRegion> = regions
        .iter()
        .filter(|r| !r.action.starts_with("track:"))
        .cloned()
        .collect();
    let mut plan = String::from("{\"buttons\":");
    plan.push_str(&regions_to_json(&buttons));
    plan.push_str(",\"layers\":[");
    if hide_tracked {
        for (i, layer) in demo::plan_layers(&regions).iter().enumerate() {
            if i > 0 {
                plan.push(',');
            }
            plan.push_str(&format!(
                "{{\"track\":\"{}\",\"x\":{:.2},\"y\":{:.2},\"width\":{:.2},\"height\":{:.2},\
                 \"spriteWidth\":{:.2},\"spriteHeight\":{:.2},\"clipWidth\":{:.2}}}",
                layer.track,
                layer.x,
                layer.y,
                layer.width,
                layer.height,
                layer.sprite_width,
                layer.sprite_height,
                layer.clip_width
            ));
        }
    }
    plan.push_str("]}");
    Some((buffer, plan))
}

fn vec_into_raw(buffer: Vec<u8>, out_len: *mut usize) -> *mut u8 {
    let mut buffer = buffer.into_boxed_slice();
    let len = buffer.len();
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    unsafe { *out_len = len };
    ptr
}

fn string_into_raw(s: String) -> *mut std::ffi::c_char {
    match std::ffi::CString::new(s) {
        Ok(cstring) => cstring.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

unsafe fn cstr<'a>(ptr: *const std::ffi::c_char) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    unsafe { std::ffi::CStr::from_ptr(ptr) }.to_str().ok()
}

/// C ABI: dispatch a `data-action` from a tapped widget element into the
/// Rust-owned widget state persisted at `state_path`.
///
/// # Safety
/// `state_path` and `action` must be valid NUL-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn blitz_widget_dispatch(
    state_path: *const std::ffi::c_char,
    action: *const std::ffi::c_char,
) {
    if let (Some(path), Some(action)) = (unsafe { cstr(state_path) }, unsafe { cstr(action) }) {
        store::dispatch(path, action);
    }
}

/// C ABI: render a complete widget frame (see [`render_widget_frame`]) for a
/// widget `kind` (`counter`, `counter-lock`, `interactive`, `anim`) at the
/// current Rust-owned state. Returns the background RGBA buffer (`*out_len`
/// bytes; free with [`blitz_buffer_free`]) and writes the JSON compositing
/// plan (buttons + layers) to `*out_plan_json` (free with
/// [`blitz_string_free`]). `clock` is a display-only time string (used by
/// `counter`; may be NULL). With `hide_tracked != 0` the `data-track`
/// elements keep their layout but don't paint, and the plan includes their
/// sprite layers for native compositing. Returns NULL for an unknown kind.
///
/// # Safety
/// All pointers must be valid; strings must be NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn blitz_widget_frame(
    state_path: *const std::ffi::c_char,
    kind: *const std::ffi::c_char,
    width: u32,
    height: u32,
    scale: f32,
    time_secs: f64,
    hide_tracked: i32,
    clock: *const std::ffi::c_char,
    out_len: *mut usize,
    out_plan_json: *mut *mut std::ffi::c_char,
) -> *mut u8 {
    if out_len.is_null() || out_plan_json.is_null() || width == 0 || height == 0 || scale <= 0.0 {
        return std::ptr::null_mut();
    }
    let (Some(path), Some(kind)) = (unsafe { cstr(state_path) }, unsafe { cstr(kind) }) else {
        return std::ptr::null_mut();
    };
    let clock = unsafe { cstr(clock) }.unwrap_or("");
    match render_widget_frame(
        path,
        kind,
        width,
        height,
        scale as f64,
        time_secs,
        hide_tracked != 0,
        clock,
    ) {
        Some((buffer, plan)) => {
            unsafe { *out_plan_json = string_into_raw(plan) };
            vec_into_raw(buffer, out_len)
        }
        None => std::ptr::null_mut(),
    }
}

/// C ABI: how many more seconds the animation widget is in motion (an
/// in-flight transition or playback) — how long the native shell should keep
/// re-rendering. 0 when settled.
///
/// # Safety
/// `state_path` must be a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn blitz_widget_refresh_secs(state_path: *const std::ffi::c_char) -> f64 {
    unsafe { cstr(state_path) }.map_or(0.0, store::refresh_secs)
}

/// C ABI: plan the animation widget's timeline as JSON —
/// `{"frames":[{"offset":..,"time":..},..]}` where both are the display
/// offset in seconds from now to render and show each frame at (pass `time`
/// as the frame's render time). One settled frame when idle; a sequence
/// covering the remaining transition/playback otherwise. Free with
/// [`blitz_string_free`].
///
/// # Safety
/// `state_path` must be a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn blitz_widget_anim_timeline_json(
    state_path: *const std::ffi::c_char,
) -> *mut std::ffi::c_char {
    match unsafe { cstr(state_path) } {
        Some(path) => string_into_raw(store::anim_timeline_json(path)),
        None => std::ptr::null_mut(),
    }
}

/// C ABI: render the standalone sprite of a `data-track` layer (from the
/// frame plan) to an RGBA8888 buffer of `*out_len` bytes at
/// `(width * scale) x (height * scale)` pixels. Free with
/// [`blitz_buffer_free`]; returns NULL for an unknown track.
///
/// # Safety
/// `track` must be a valid NUL-terminated string and `out_len` a valid
/// pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn blitz_widget_sprite(
    track: *const std::ffi::c_char,
    width: u32,
    height: u32,
    scale: f32,
    out_len: *mut usize,
) -> *mut u8 {
    if out_len.is_null() || width == 0 || height == 0 || scale <= 0.0 {
        return std::ptr::null_mut();
    }
    match unsafe { cstr(track) }.and_then(demo::sprite_html) {
        Some(html) => vec_into_raw(
            render_html_to_rgba(&html, width, height, scale as f64),
            out_len,
        ),
        None => std::ptr::null_mut(),
    }
}

/// C ABI: free a buffer returned by [`blitz_render_html`] or
/// [`blitz_render_html_with_regions`].
///
/// # Safety
/// `ptr`/`len` must come from a single previous render call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn blitz_buffer_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)) });
    }
}

/// C ABI: free a string returned by this library.
///
/// # Safety
/// `ptr` must come from a previous call returning a string from this library.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn blitz_string_free(ptr: *mut std::ffi::c_char) {
    if !ptr.is_null() {
        drop(unsafe { std::ffi::CString::from_raw(ptr) });
    }
}

#[cfg(target_os = "android")]
mod android {
    use jni::JNIEnv;
    use jni::objects::{JByteArray, JClass, JString};
    use jni::sys::{jdouble, jfloat, jint};

    /// JNI: `BlitzRenderer.dispatch(String, String)` — apply a `data-action`
    /// to the Rust-owned widget state persisted at `statePath`.
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_dev_dioxus_blitzwidget_BlitzRenderer_dispatch<'l>(
        mut env: JNIEnv<'l>,
        _class: JClass<'l>,
        state_path: JString<'l>,
        action: JString<'l>,
    ) {
        let (Ok(path), Ok(action)) = (env.get_string(&state_path), env.get_string(&action)) else {
            return;
        };
        let (path, action): (String, String) = (path.into(), action.into());
        crate::store::dispatch(&path, &action);
    }

    /// JNI: `BlitzRenderer.renderWidget(String, String, int, int, float, double, String): byte[]`
    /// — RGBA8888 pixels of a widget kind's frame at the current Rust-owned
    /// state, sampled at `timeSecs` on the animation clock.
    #[unsafe(no_mangle)]
    #[allow(clippy::too_many_arguments)]
    pub extern "system" fn Java_dev_dioxus_blitzwidget_BlitzRenderer_renderWidget<'l>(
        mut env: JNIEnv<'l>,
        _class: JClass<'l>,
        state_path: JString<'l>,
        kind: JString<'l>,
        width: jint,
        height: jint,
        scale: jfloat,
        time_secs: jdouble,
        clock: JString<'l>,
    ) -> JByteArray<'l> {
        let (Ok(path), Ok(kind)) = (env.get_string(&state_path), env.get_string(&kind)) else {
            return JByteArray::default();
        };
        let (path, kind): (String, String) = (path.into(), kind.into());
        let clock: String = env.get_string(&clock).map(Into::into).unwrap_or_default();
        match crate::render_widget_frame(
            &path,
            &kind,
            width as u32,
            height as u32,
            scale as f64,
            time_secs,
            false,
            &clock,
        ) {
            Some((buffer, _)) => env
                .byte_array_from_slice(&buffer)
                .unwrap_or_else(|_| JByteArray::default()),
            None => JByteArray::default(),
        }
    }

    /// JNI: `BlitzRenderer.widgetPlan(String, String, int, int, float): String`
    /// — the JSON compositing plan (`{"buttons":[..],"layers":[..]}`,
    /// coordinates in CSS px / dp) of a widget kind's frame at the current
    /// Rust-owned state.
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_dev_dioxus_blitzwidget_BlitzRenderer_widgetPlan<'l>(
        mut env: JNIEnv<'l>,
        _class: JClass<'l>,
        state_path: JString<'l>,
        kind: JString<'l>,
        width: jint,
        height: jint,
        scale: jfloat,
    ) -> JString<'l> {
        let (Ok(path), Ok(kind)) = (env.get_string(&state_path), env.get_string(&kind)) else {
            return JString::default();
        };
        let (path, kind): (String, String) = (path.into(), kind.into());
        match crate::render_widget_frame(
            &path,
            &kind,
            width as u32,
            height as u32,
            scale as f64,
            0.0,
            false,
            "",
        ) {
            Some((_, plan)) => env.new_string(plan).unwrap_or_else(|_| JString::default()),
            None => JString::default(),
        }
    }

    /// JNI: `BlitzRenderer.refreshSecs(String): double` — how many more
    /// seconds the animation widget is in motion (an in-flight transition or
    /// playback); how long the provider should keep re-rendering. 0 when
    /// settled.
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_dev_dioxus_blitzwidget_BlitzRenderer_refreshSecs<'l>(
        mut env: JNIEnv<'l>,
        _class: JClass<'l>,
        state_path: JString<'l>,
    ) -> jdouble {
        match env.get_string(&state_path) {
            Ok(path) => {
                let path: String = path.into();
                crate::store::refresh_secs(&path)
            }
            Err(_) => 0.0,
        }
    }
}
