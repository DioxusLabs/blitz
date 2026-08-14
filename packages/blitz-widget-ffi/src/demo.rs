//! Shared demo widget template: an interactive counter plus a discrete
//! "slider" built from tappable segments. Each tappable element carries a
//! `data-action` attribute; the native widget shells overlay one tap target
//! per extracted hit region.

/// Simple counter widget (home screen). The whole card is one tap target
/// (`data-action="count"`). `clock` is a display-only time string supplied by
/// the shell.
pub fn counter_html(count: i32, clock: &str) -> String {
    format!(
        r##"<!DOCTYPE html>
<html><head><style>
  body {{ margin: 0; font-family: sans-serif; }}
  .card {{
    box-sizing: border-box; width: 100%; height: 100vh; padding: 14px;
    background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
    color: white; display: flex; flex-direction: column;
    justify-content: space-between;
  }}
  .row {{ display: flex; justify-content: space-between; align-items: center; }}
  .title {{ font-size: 13px; font-weight: 600; opacity: 0.9; }}
  .time {{ font-size: 11px; opacity: 0.7; }}
  .count {{ font-size: 48px; font-weight: bold; text-align: center; }}
  .hint {{
    font-size: 11px; text-align: center; opacity: 0.85;
    background: rgba(255,255,255,0.18); border-radius: 10px; padding: 5px 8px;
  }}
</style></head>
<body><div class="card" data-action="count">
  <div class="row">
    <div class="title">⚡ Blitz Counter</div>
    <div class="time">{clock}</div>
  </div>
  <div class="count">{count}</div>
  <div class="hint">Tap to increment · HTML by Blitz</div>
</div></body></html>"##
    )
}

/// Simple counter widget (lock screen accessory).
pub fn counter_lock_html(count: i32) -> String {
    format!(
        r##"<!DOCTYPE html>
<html><head><style>
  body {{ margin: 0; font-family: sans-serif; }}
  .card {{
    box-sizing: border-box; width: 100%; height: 100vh; padding: 8px 12px;
    color: white; display: flex; align-items: center; gap: 10px;
  }}
  .count {{ font-size: 32px; font-weight: bold; }}
  .label {{ font-size: 12px; line-height: 1.3; opacity: 0.9; }}
</style></head>
<body><div class="card" data-action="count">
  <div class="count">{count}</div>
  <div class="label">Blitz Counter<br>HTML/CSS render</div>
</div></body></html>"##
    )
}

/// Actions emitted by this template: `incr`, `decr`, `reset`, `slider:N`
/// (N in 0..=10).
pub fn widget_html(count: i32, slider: i32) -> String {
    let slider = slider.clamp(0, 10);
    let percent = slider * 10;

    let mut segments = String::new();
    for i in 0..=10 {
        let filled = if i <= slider { " filled" } else { "" };
        segments.push_str(&format!(
            r#"<div class="seg{filled}" data-action="slider:{i}"></div>"#
        ));
    }

    format!(
        r##"<!DOCTYPE html>
<html><head><style>
  body {{ margin: 0; font-family: sans-serif; }}
  .card {{
    box-sizing: border-box; width: 100%; height: 100vh; padding: 12px 14px;
    background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
    color: white; display: flex; flex-direction: column;
    justify-content: space-between;
  }}
  .row {{ display: flex; justify-content: space-between; align-items: center; }}
  .title {{ font-size: 13px; font-weight: 600; opacity: 0.9; }}
  .reset {{
    font-size: 11px; font-weight: 600; padding: 4px 10px;
    background: rgba(255,255,255,0.18); border-radius: 9px;
  }}
  .counter {{ display: flex; align-items: center; justify-content: center; gap: 18px; }}
  .btn {{
    width: 40px; height: 40px; border-radius: 20px;
    background: rgba(255,255,255,0.22);
    display: flex; align-items: center; justify-content: center;
    font-size: 24px; font-weight: bold;
  }}
  .count {{ font-size: 40px; font-weight: bold; min-width: 70px; text-align: center; }}
  .slider-label {{ font-size: 11px; opacity: 0.85; margin-bottom: 4px;
    display: flex; justify-content: space-between; }}
  .track {{ display: flex; gap: 3px; height: 18px; }}
  .seg {{
    flex: 1 1 0; border-radius: 4px; background: rgba(255,255,255,0.22);
  }}
  .seg.filled {{ background: #ffd76e; }}
</style></head>
<body><div class="card">
  <div class="row">
    <div class="title">Blitz Widget · HTML/CSS</div>
    <div class="reset" data-action="reset">Reset</div>
  </div>
  <div class="counter">
    <div class="btn" data-action="decr">−</div>
    <div class="count">{count}</div>
    <div class="btn" data-action="incr">+</div>
  </div>
  <div>
    <div class="slider-label"><span>Brightness</span><span>{percent}%</span></div>
    <div class="track">{segments}</div>
  </div>
</div></body></html>"##
    )
}

/// Total length of the animation cycle sampled by the scrubber, in seconds.
pub const ANIMATION_DURATION: f64 = 4.0;

/// Animated demo template: CSS `@keyframes` animations (a bouncing ball, a
/// progress sweep and a hue-shifting badge) plus a scrubber whose segments
/// carry `time:N` actions (N in 0..=10, mapping to N/10 of
/// [`ANIMATION_DURATION`]), a `step` action and a `play` action. The native
/// shell picks the sampled instant via the `time` render parameter — the HTML
/// itself is identical for every frame.
///
/// The ball and progress fill animate *layout* properties (`left`, `width`,
/// `height`, `top`), so their animated rects are extractable via
/// `data-track` hit regions at any sampled instant. With
/// `hide_tracked = true` they keep their layout but don't paint
/// (`visibility: hidden`): the native shell composites them as separate
/// layers positioned from the extracted rects, letting the platform tween
/// position/size between samples at full frame rate.
pub fn animated_html(scrub: i32, hide_tracked: bool) -> String {
    let scrub = scrub.clamp(0, 10);
    let percent = scrub * 10;
    let ball_extra = if hide_tracked {
        " visibility: hidden;"
    } else {
        ""
    };
    let fill_extra = ball_extra;

    let mut segments = String::new();
    for i in 0..=10 {
        let filled = if i <= scrub { " filled" } else { "" };
        segments.push_str(&format!(
            r#"<div class="seg{filled}" data-action="time:{i}"></div>"#
        ));
    }

    format!(
        r##"<!DOCTYPE html>
<html><head><style>
  body {{ margin: 0; font-family: sans-serif; }}
  .card {{
    box-sizing: border-box; width: 100%; height: 100vh; padding: 12px 14px;
    background: linear-gradient(135deg, #0f2027 0%, #2c5364 100%);
    color: white; display: flex; flex-direction: column;
    justify-content: space-between;
  }}
  .row {{ display: flex; justify-content: space-between; align-items: center; }}
  .title {{ font-size: 13px; font-weight: 600; opacity: 0.9; }}
  .badge {{
    font-size: 11px; font-weight: 600; padding: 4px 10px; border-radius: 9px;
    background-color: #e66aa8;
    animation: hue 4s linear infinite;
  }}
  .stage {{ position: relative; height: 52px; margin: 4px 0; }}
  .ball {{
    position: absolute; top: 9px; left: 0;
    width: 34px; height: 34px; border-radius: 50%;
    background: #ffd76e;
    animation: bounce 4s ease-in-out infinite;{ball_extra}
  }}
  .rail {{ position: relative; height: 8px; border-radius: 4px;
    background: rgba(255,255,255,0.18); margin-bottom: 8px; }}
  .fill {{ position: absolute; top: 0; left: 0; height: 8px;
    border-radius: 4px; background: #5fd0a5;
    animation: sweep 4s linear infinite;{fill_extra} }}
  .scrub-label {{ font-size: 11px; opacity: 0.85; margin-bottom: 4px;
    display: flex; justify-content: space-between; }}
  .track {{ display: flex; gap: 3px; height: 18px; }}
  .seg {{ flex: 1 1 0; border-radius: 4px; background: rgba(255,255,255,0.22); }}
  .seg.filled {{ background: #ffd76e; }}
  .badge.play {{ animation: none; background-color: rgba(255,255,255,0.18); }}
  @keyframes bounce {{
    0%   {{ left: 0px;   top: 9px; width: 34px; height: 34px; }}
    50%  {{ left: 280px; top: 0px; width: 51px; height: 51px; }}
    100% {{ left: 0px;   top: 9px; width: 34px; height: 34px; }}
  }}
  @keyframes sweep {{
    from {{ width: 0%; }}
    to   {{ width: 100%; }}
  }}
  @keyframes hue {{
    0%   {{ background-color: #e66aa8; }}
    33%  {{ background-color: #5fd0a5; }}
    66%  {{ background-color: #667eea; }}
    100% {{ background-color: #e66aa8; }}
  }}
</style></head>
<body><div class="card">
  <div class="row">
    <div class="title">Blitz CSS Animation</div>
    <div class="row" style="gap: 6px;">
      <div class="badge play" data-action="play">Play</div>
      <div class="badge" data-action="step">Step +0.4s</div>
    </div>
  </div>
  <div class="stage"><div class="ball" data-track="ball"></div></div>
  <div class="rail" data-track="rail"><div class="fill" data-track="fill"></div></div>
  <div>
    <div class="scrub-label"><span>Timeline scrubber</span><span>t = {percent}%</span></div>
    <div class="track">{segments}</div>
  </div>
</div></body></html>"##
    )
}

/// Maximum animated size of the bounce ball (the 50% keyframe), used as the
/// sprite render size so one sprite covers every animated size.
pub const BALL_MAX_SIZE: f64 = 51.0;

/// A native compositing layer planned by Rust from the resolved `data-track`
/// rects: the shell renders the `track`'s sprite at
/// `sprite_width` x `sprite_height`, draws it at the layer rect, and clips it
/// to `clip_width` from the left.
#[derive(Debug, Clone)]
pub struct LayerPlan {
    pub track: &'static str,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub sprite_width: f64,
    pub sprite_height: f64,
    pub clip_width: f64,
}

/// Plan the native compositing layers of the animated widget from the
/// resolved track rects: the ball is drawn at its animated rect; the fill
/// sprite spans the rail's rect, clipped to the fill's animated width.
pub fn plan_layers(regions: &[crate::HitRegion]) -> Vec<LayerPlan> {
    let rect = |name: &str| regions.iter().find(|r| r.action == format!("track:{name}"));
    let mut layers = Vec::new();
    if let (Some(rail), Some(fill)) = (rect("rail"), rect("fill")) {
        layers.push(LayerPlan {
            track: "fill",
            x: rail.x,
            y: rail.y,
            width: rail.width,
            height: rail.height,
            sprite_width: rail.width,
            sprite_height: rail.height,
            clip_width: fill.width,
        });
    }
    if let Some(ball) = rect("ball") {
        layers.push(LayerPlan {
            track: "ball",
            x: ball.x,
            y: ball.y,
            width: ball.width,
            height: ball.height,
            sprite_width: BALL_MAX_SIZE,
            sprite_height: BALL_MAX_SIZE,
            clip_width: ball.width,
        });
    }
    layers
}

/// Standalone sprite HTML for a `data-track` element, so the native shell can
/// composite tracked elements as separately positioned layers without knowing
/// what they are: it renders the sprite for each track name Rust reports and
/// scales/positions it from the tracked rect. `ball` is rendered at its
/// maximum animated size; `fill` at the rail's full width (the shell clips it
/// to the tracked rect's animated width).
pub fn sprite_html(track: &str) -> Option<String> {
    let body = match track {
        "ball" => {
            r##".sprite { width: 100vw; height: 100vh; border-radius: 50%;
    background: #ffd76e; }"##
        }
        "fill" => {
            r##".sprite { width: 100vw; height: 100vh; border-radius: 4px;
    background: #5fd0a5; }"##
        }
        _ => return None,
    };
    Some(format!(
        r##"<!DOCTYPE html>
<html><head><style>
  body {{ margin: 0; }}
  {body}
</style></head>
<body><div class="sprite"></div></body></html>"##
    ))
}
