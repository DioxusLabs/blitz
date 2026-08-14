//! Rust-owned widget state and action dispatch.
//!
//! All widget state (counters, slider, animation clock, playback) lives here,
//! persisted to a small key=value file whose path the native shell supplies
//! (its app container). The native side is a dumb shell: it forwards tap
//! actions to [`dispatch`], asks for the HTML/timeline for a widget kind, and
//! blits the rendered frames — no widget logic or state lives in Swift/Java.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::demo;

/// Seconds of flip-book playback triggered by the `play` action.
pub const PLAY_SECS: f64 = 8.0;

#[derive(Debug, Clone, PartialEq)]
pub struct WidgetState {
    /// Simple counter widget value.
    pub count: i32,
    /// Interactive demo counter value.
    pub demo_count: i32,
    /// Interactive demo slider segment (0..=10).
    pub slider: i32,
    /// CSS animation clock of the animation widget, in seconds.
    pub anim_time: f64,
    /// Epoch seconds when `play` was last dispatched (0 = not playing).
    pub play_started: f64,
}

impl Default for WidgetState {
    fn default() -> Self {
        Self {
            count: 0,
            demo_count: 0,
            slider: 5,
            anim_time: 0.0,
            play_started: 0.0,
        }
    }
}

pub fn load(path: &str) -> WidgetState {
    let mut state = WidgetState::default();
    let Ok(contents) = std::fs::read_to_string(path) else {
        return state;
    };
    for line in contents.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "count" => state.count = value.parse().unwrap_or(state.count),
            "demo_count" => state.demo_count = value.parse().unwrap_or(state.demo_count),
            "slider" => state.slider = value.parse().unwrap_or(state.slider),
            "anim_time" => state.anim_time = value.parse().unwrap_or(state.anim_time),
            "play_started" => state.play_started = value.parse().unwrap_or(state.play_started),
            _ => {}
        }
    }
    state
}

pub fn save(path: &str, state: &WidgetState) {
    if let Some(parent) = Path::new(path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let contents = format!(
        "count={}\ndemo_count={}\nslider={}\nanim_time={}\nplay_started={}\n",
        state.count, state.demo_count, state.slider, state.anim_time, state.play_started
    );
    let _ = std::fs::write(path, contents);
}

fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Apply a `data-action` from any of the demo widgets to the persisted state.
pub fn dispatch(path: &str, action: &str) {
    let mut state = load(path);
    match action {
        "count" => state.count += 1,
        "incr" => state.demo_count += 1,
        "decr" => state.demo_count -= 1,
        "reset" => {
            state.demo_count = 0;
            state.slider = 5;
        }
        "step" => {
            state.anim_time = (state.anim_time + 0.4) % (demo::ANIMATION_DURATION + 0.0001);
        }
        "play" => state.play_started = now_epoch(),
        _ => {
            if let Some(value) = action.strip_prefix("slider:")
                && let Ok(value) = value.parse::<i32>()
            {
                state.slider = value.clamp(0, 10);
            } else if let Some(seg) = action.strip_prefix("time:")
                && let Ok(seg) = seg.parse::<i32>()
            {
                state.anim_time = seg.clamp(0, 10) as f64 / 10.0 * demo::ANIMATION_DURATION;
            }
        }
    }
    save(path, &state);
}

/// The scrubber segment highlighted for the current animation clock.
pub fn scrub_segment(state: &WidgetState) -> i32 {
    (state.anim_time / demo::ANIMATION_DURATION * 10.0).round() as i32
}

/// Build the HTML for a widget kind at the current state. Kinds: `counter`
/// (home screen; `clock` is a display-only time string), `counter-lock`
/// (lock screen), `interactive` (counter + slider), `anim` (CSS animation
/// demo; `hide_tracked` leaves the `data-track` elements as invisible layout
/// placeholders for native layer compositing).
pub fn widget_html(path: &str, kind: &str, hide_tracked: bool, clock: &str) -> Option<String> {
    let state = load(path);
    match kind {
        "counter" => Some(demo::counter_html(state.count, clock)),
        "counter-lock" => Some(demo::counter_lock_html(state.count)),
        "interactive" => Some(demo::widget_html(state.demo_count, state.slider)),
        "anim" => Some(demo::animated_html(scrub_segment(&state), hide_tracked)),
        _ => None,
    }
}

/// The current animation clock, in seconds.
pub fn anim_time(path: &str) -> f64 {
    load(path).anim_time
}

/// The animation clock `elapsed` seconds into playback from the current
/// state (wraps around the animation cycle).
pub fn anim_time_at(path: &str, elapsed: f64) -> f64 {
    (load(path).anim_time + elapsed) % demo::ANIMATION_DURATION
}

/// Plan the animation widget's timeline as JSON:
/// `{"frames":[{"offset":..,"time":..},..]}` where `offset` is seconds
/// relative to now at which to show the frame and `time` is the animation
/// clock to render it at.
///
/// Normally this is a single frame at the current clock. Right after a
/// `play` dispatch it is a flip-book covering [`PLAY_SECS`] of playback at
/// 1s spacing; the first frame (the pose already on screen) is backdated so
/// the first moving frame lands just after now. Consuming the plan clears
/// the pending playback (one-shot).
pub fn anim_timeline_json(path: &str) -> String {
    let mut state = load(path);
    let playing = state.play_started > 0.0 && now_epoch() - state.play_started < PLAY_SECS;
    let mut json = String::from("{\"frames\":[");
    if playing {
        state.play_started = 0.0;
        save(path, &state);
        for i in 0..=(PLAY_SECS as i32) {
            if i > 0 {
                json.push(',');
            }
            let time = (state.anim_time + i as f64) % demo::ANIMATION_DURATION;
            json.push_str(&format!(
                "{{\"offset\":{:.2},\"time\":{:.4}}}",
                i as f64 - 0.8,
                time
            ));
        }
    } else {
        json.push_str(&format!("{{\"offset\":0,\"time\":{:.4}}}", state.anim_time));
    }
    json.push_str("]}");
    json
}
