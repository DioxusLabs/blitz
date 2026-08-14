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

/// Seconds of playback triggered by the `play` action (two full cycles, so
/// the clock lands back where it started).
pub const PLAY_SECS: f64 = 8.0;

#[derive(Debug, Clone, PartialEq)]
pub struct WidgetState {
    /// Simple counter widget value.
    pub count: i32,
    /// Interactive demo counter value.
    pub demo_count: i32,
    /// Interactive demo slider segment (0..=10).
    pub slider: i32,
    /// Target of the animation widget's clock, in seconds of the cycle. The
    /// widget transitions toward the pose at this clock.
    pub anim_time: f64,
    /// Epoch seconds when `play` was last dispatched (0 = not playing).
    pub play_started: f64,
    /// Serialized [`demo::Pose`] the in-flight transition started from
    /// (empty = start at the target pose).
    pub trans_from: String,
    /// Epoch seconds when the in-flight transition started (0 = none).
    pub trans_start: f64,
}

impl Default for WidgetState {
    fn default() -> Self {
        Self {
            count: 0,
            demo_count: 0,
            slider: 5,
            anim_time: 0.0,
            play_started: 0.0,
            trans_from: String::new(),
            trans_start: 0.0,
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
            "trans_from" => state.trans_from = value.to_string(),
            "trans_start" => state.trans_start = value.parse().unwrap_or(state.trans_start),
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
        "count={}\ndemo_count={}\nslider={}\nanim_time={}\nplay_started={}\ntrans_from={}\ntrans_start={}\n",
        state.count,
        state.demo_count,
        state.slider,
        state.anim_time,
        state.play_started,
        state.trans_from,
        state.trans_start
    );
    let _ = std::fs::write(path, contents);
}

pub(crate) fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Whether `play` playback is running at wall-clock `now`.
pub(crate) fn playing(state: &WidgetState, now: f64) -> bool {
    state.play_started > 0.0 && now - state.play_started < PLAY_SECS
}

/// The pose the animation widget is transitioning toward at wall-clock
/// `now`: the pose at the target clock — which advances in real time during
/// playback, so the transition chases a moving target and playback itself
/// eases in from wherever the widget was.
pub(crate) fn target_pose(state: &WidgetState, now: f64) -> demo::Pose {
    let clock = if playing(state, now) {
        state.anim_time + (now - state.play_started)
    } else {
        state.anim_time
    };
    demo::pose_at(clock)
}

/// The pose the in-flight transition started from.
pub(crate) fn from_pose(state: &WidgetState) -> demo::Pose {
    demo::Pose::parse(&state.trans_from).unwrap_or_else(|| demo::pose_at(state.anim_time))
}

/// Re-baseline the animation widget's transition before changing its target:
/// the pose currently on screen (mid-transition or mid-playback) becomes the
/// `from` pose of a fresh transition starting now — so an interrupting action
/// eases from wherever the widget is, never snapping.
fn rebase_transition(state: &mut WidgetState, now: f64) {
    state.trans_from = crate::current_anim_pose(state, now).serialize();
    state.trans_start = now;
    if playing(state, now) {
        // Fold the played time into the clock so the target stays continuous
        // when playback is interrupted.
        state.anim_time = (state.anim_time + (now - state.play_started)) % demo::ANIMATION_DURATION;
    }
    state.play_started = 0.0;
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
            let now = now_epoch();
            rebase_transition(&mut state, now);
            state.anim_time = (state.anim_time + 0.4) % (demo::ANIMATION_DURATION + 0.0001);
        }
        "play" => {
            let now = now_epoch();
            rebase_transition(&mut state, now);
            state.play_started = now;
        }
        _ => {
            if let Some(value) = action.strip_prefix("slider:")
                && let Ok(value) = value.parse::<i32>()
            {
                state.slider = value.clamp(0, 10);
            } else if let Some(seg) = action.strip_prefix("time:")
                && let Ok(seg) = seg.parse::<i32>()
            {
                let now = now_epoch();
                rebase_transition(&mut state, now);
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
/// (lock screen), `interactive` (counter + slider). The `anim` kind is
/// rendered by [`crate::render_widget_frame`] directly (its document is
/// mutated mid-render to drive CSS transitions).
pub fn widget_html(path: &str, kind: &str, _hide_tracked: bool, clock: &str) -> Option<String> {
    let state = load(path);
    match kind {
        "counter" => Some(demo::counter_html(state.count, clock)),
        "counter-lock" => Some(demo::counter_lock_html(state.count)),
        "interactive" => Some(demo::widget_html(state.demo_count, state.slider)),
        _ => None,
    }
}

/// How many more seconds the animation widget is in motion (an in-flight
/// transition or playback) — how long the native shell should keep
/// re-rendering. 0 when settled.
pub fn refresh_secs(path: &str) -> f64 {
    let state = load(path);
    let now = now_epoch();
    let trans_left = if state.trans_start > 0.0 {
        state.trans_start + demo::TRANSITION_SECS - now
    } else {
        0.0
    };
    let play_left = if playing(&state, now) {
        state.play_started + PLAY_SECS - now
    } else {
        0.0
    };
    trans_left.max(play_left).max(0.0)
}

/// Plan the animation widget's timeline as JSON:
/// `{"frames":[{"offset":..,"time":..},..]}` where both fields are the
/// display offset in seconds from now (`time` is what to pass as the frame's
/// render time — the renderer maps display offsets onto the persisted
/// transition/playback clocks itself).
///
/// One settled frame when idle. While in motion, frames span the remaining
/// transition (0.5s spacing) or playback (1s spacing), plus a final settled
/// frame; the first frame (the pose already on screen) is backdated so the
/// first moving frame lands just after now.
pub fn anim_timeline_json(path: &str) -> String {
    let state = load(path);
    let now = now_epoch();
    let remaining = refresh_secs(path);
    let spacing = if playing(&state, now) { 1.0 } else { 0.5 };
    let mut json = String::from("{\"frames\":[");
    if remaining <= 0.0 {
        json.push_str("{\"offset\":0,\"time\":0}");
    } else {
        let count = (remaining / spacing).ceil() as i32 + 1;
        for i in 0..=count {
            if i > 0 {
                json.push(',');
            }
            let t = (i as f64 * spacing).min(remaining + spacing);
            let offset = if i == 0 { -0.8 } else { t };
            json.push_str(&format!("{{\"offset\":{:.2},\"time\":{:.4}}}", offset, t));
        }
    }
    json.push_str("]}");
    json
}
