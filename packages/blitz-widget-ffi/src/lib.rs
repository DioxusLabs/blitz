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

use anyrender::{PaintScene as _, render_to_buffer};
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{BaseDocument, DocumentConfig, NodeId, util::Color};
use blitz_html::HtmlDocument;
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

    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
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
    );

    (buffer, regions)
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

/// C ABI: render `html` (NUL-terminated UTF-8) to an RGBA8888 buffer.
///
/// Returns a pointer to a heap-allocated buffer of `*out_len` bytes
/// (`(width * scale) * (height * scale) * 4`). Free it with
/// [`blitz_buffer_free`]. Returns NULL on invalid input.
///
/// # Safety
/// `html` must be a valid NUL-terminated string and `out_len` a valid pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn blitz_render_html(
    html: *const std::ffi::c_char,
    width: u32,
    height: u32,
    scale: f32,
    out_len: *mut usize,
) -> *mut u8 {
    if html.is_null() || out_len.is_null() || width == 0 || height == 0 || scale <= 0.0 {
        return std::ptr::null_mut();
    }
    let html = match unsafe { std::ffi::CStr::from_ptr(html) }.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    vec_into_raw(
        render_html_to_rgba(html, width, height, scale as f64),
        out_len,
    )
}

/// C ABI: like [`blitz_render_html`], but samples CSS animations/transitions
/// at `time_secs` on the document's animation clock (animations start at t=0).
///
/// # Safety
/// `html` must be a valid NUL-terminated string and `out_len` a valid pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn blitz_render_html_at(
    html: *const std::ffi::c_char,
    width: u32,
    height: u32,
    scale: f32,
    time_secs: f64,
    out_len: *mut usize,
) -> *mut u8 {
    if html.is_null() || out_len.is_null() || width == 0 || height == 0 || scale <= 0.0 {
        return std::ptr::null_mut();
    }
    let html = match unsafe { std::ffi::CStr::from_ptr(html) }.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let (buffer, _) = render_html(html, width, height, scale as f64, time_secs);
    vec_into_raw(buffer, out_len)
}

/// C ABI: like [`blitz_render_html`], but additionally writes a JSON array of
/// hit regions (`[{"action":..,"x":..,"y":..,"width":..,"height":..}, ..]`,
/// coordinates in CSS px / points) for all elements with a `data-action`
/// attribute to `*out_regions_json`. Free the JSON with [`blitz_string_free`].
///
/// # Safety
/// All pointers must be valid; `html` must be NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn blitz_render_html_with_regions(
    html: *const std::ffi::c_char,
    width: u32,
    height: u32,
    scale: f32,
    time_secs: f64,
    out_len: *mut usize,
    out_regions_json: *mut *mut std::ffi::c_char,
) -> *mut u8 {
    if html.is_null()
        || out_len.is_null()
        || out_regions_json.is_null()
        || width == 0
        || height == 0
        || scale <= 0.0
    {
        return std::ptr::null_mut();
    }
    let html = match unsafe { std::ffi::CStr::from_ptr(html) }.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let (buffer, regions) = render_html(html, width, height, scale as f64, time_secs);
    unsafe { *out_regions_json = string_into_raw(regions_to_json(&regions)) };
    vec_into_raw(buffer, out_len)
}

/// C ABI: build the demo widget HTML (counter + slider) for the given state.
/// Free the returned string with [`blitz_string_free`].
#[unsafe(no_mangle)]
pub extern "C" fn blitz_demo_widget_html(count: i32, slider: i32) -> *mut std::ffi::c_char {
    string_into_raw(demo::widget_html(count, slider))
}

/// C ABI: build the animated demo widget HTML (CSS keyframe animations plus a
/// time scrubber). `scrub` is the highlighted scrubber segment (0..=10);
/// sample the animations via the `time_secs` render parameter. With
/// `hide_tracked != 0` the `data-track` elements keep their layout but don't
/// paint, for native layer compositing. Free the returned string with
/// [`blitz_string_free`].
#[unsafe(no_mangle)]
pub extern "C" fn blitz_demo_animated_html(scrub: i32, hide_tracked: i32) -> *mut std::ffi::c_char {
    string_into_raw(demo::animated_html(scrub, hide_tracked != 0))
}

/// C ABI: standalone ball sprite HTML for native layer compositing. Free with
/// [`blitz_string_free`].
#[unsafe(no_mangle)]
pub extern "C" fn blitz_demo_ball_sprite_html() -> *mut std::ffi::c_char {
    string_into_raw(demo::ball_sprite_html())
}

/// C ABI: standalone progress-fill sprite HTML for native layer compositing.
/// Free with [`blitz_string_free`].
#[unsafe(no_mangle)]
pub extern "C" fn blitz_demo_fill_sprite_html() -> *mut std::ffi::c_char {
    string_into_raw(demo::fill_sprite_html())
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
    use jni::sys::{jboolean, jdouble, jfloat, jint};

    /// JNI: `dev.dioxus.blitzwidget.BlitzRenderer.renderHtml(String, int, int, float): byte[]`
    /// Returns RGBA8888 pixels suitable for `Bitmap.copyPixelsFromBuffer` on an
    /// ARGB_8888 bitmap of `(width * scale) x (height * scale)` pixels.
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_dev_dioxus_blitzwidget_BlitzRenderer_renderHtml<'l>(
        mut env: JNIEnv<'l>,
        _class: JClass<'l>,
        html: JString<'l>,
        width: jint,
        height: jint,
        scale: jfloat,
    ) -> JByteArray<'l> {
        let html: String = match env.get_string(&html) {
            Ok(s) => s.into(),
            Err(_) => return JByteArray::default(),
        };
        let buffer = crate::render_html_to_rgba(&html, width as u32, height as u32, scale as f64);
        env.byte_array_from_slice(&buffer)
            .unwrap_or_else(|_| JByteArray::default())
    }

    /// JNI: `BlitzRenderer.extractRegions(String, int, int, float): String`
    /// Returns a JSON array of hit regions (coordinates in CSS px / dp) for
    /// all elements with a `data-action` attribute.
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_dev_dioxus_blitzwidget_BlitzRenderer_extractRegions<'l>(
        mut env: JNIEnv<'l>,
        _class: JClass<'l>,
        html: JString<'l>,
        width: jint,
        height: jint,
        scale: jfloat,
    ) -> JString<'l> {
        let html: String = match env.get_string(&html) {
            Ok(s) => s.into(),
            Err(_) => return JString::default(),
        };
        let (_, regions) =
            crate::render_html(&html, width as u32, height as u32, scale as f64, 0.0);
        let json = crate::regions_to_json(&regions);
        env.new_string(json).unwrap_or_else(|_| JString::default())
    }

    /// JNI: `BlitzRenderer.demoWidgetHtml(int, int): String`
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_dev_dioxus_blitzwidget_BlitzRenderer_demoWidgetHtml<'l>(
        env: JNIEnv<'l>,
        _class: JClass<'l>,
        count: jint,
        slider: jint,
    ) -> JString<'l> {
        let html = crate::demo::widget_html(count, slider);
        env.new_string(html).unwrap_or_else(|_| JString::default())
    }

    /// JNI: `BlitzRenderer.renderHtmlAt(String, int, int, float, double): byte[]`
    /// Like `renderHtml`, but samples CSS animations at `timeSecs`.
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_dev_dioxus_blitzwidget_BlitzRenderer_renderHtmlAt<'l>(
        mut env: JNIEnv<'l>,
        _class: JClass<'l>,
        html: JString<'l>,
        width: jint,
        height: jint,
        scale: jfloat,
        time_secs: jdouble,
    ) -> JByteArray<'l> {
        let html: String = match env.get_string(&html) {
            Ok(s) => s.into(),
            Err(_) => return JByteArray::default(),
        };
        let (buffer, _) =
            crate::render_html(&html, width as u32, height as u32, scale as f64, time_secs);
        env.byte_array_from_slice(&buffer)
            .unwrap_or_else(|_| JByteArray::default())
    }

    /// JNI: `BlitzRenderer.demoAnimatedHtml(int, boolean): String`
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_dev_dioxus_blitzwidget_BlitzRenderer_demoAnimatedHtml<'l>(
        env: JNIEnv<'l>,
        _class: JClass<'l>,
        scrub: jint,
        hide_tracked: jboolean,
    ) -> JString<'l> {
        let html = crate::demo::animated_html(scrub, hide_tracked != 0);
        env.new_string(html).unwrap_or_else(|_| JString::default())
    }
}
