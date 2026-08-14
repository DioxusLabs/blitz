//! Smoke test for CSS-transition-driven widget state changes, including
//! interruption: scrub the animated widget to t=100%, sample the transition
//! mid-flight, interrupt it back to t=0%, and verify the new transition eases
//! from the interrupted pose instead of snapping. Writes one PNG per frame.
//! Usage: cargo run -p blitz-widget-ffi --example animation_frames -- outdir

use blitz_widget_ffi::{demo, render_widget_frame, store};

fn ball_x(plan: &str) -> f64 {
    // The ball layer's x from the frame plan JSON.
    let layers = plan.split("\"layers\":").nth(1).unwrap_or("");
    let ball = layers.split("\"track\":\"ball\"").nth(1).unwrap_or("");
    ball.split("\"x\":")
        .nth(1)
        .and_then(|s| s.split([',', '}']).next())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(-1.0)
}

fn main() {
    let outdir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "anim_frames".into());
    std::fs::create_dir_all(&outdir).unwrap();
    let state_path = format!("{outdir}/state.txt");
    let _ = std::fs::remove_file(&state_path);

    let (width, height, scale) = (360u32, 170u32, 3.0f64);
    let mut frame_idx = 0;
    let mut render = |label: &str, offset: f64| {
        let start = std::time::Instant::now();
        let (buffer, plan) =
            render_widget_frame(&state_path, "anim", width, height, scale, offset, true, "")
                .unwrap();
        println!(
            "{label}: offset={offset:.2}s render={:.1}ms ball_x={:.1}",
            start.elapsed().as_secs_f64() * 1000.0,
            ball_x(&plan)
        );
        let (pw, ph) = (
            (width as f64 * scale) as u32,
            (height as f64 * scale) as u32,
        );
        let file = std::fs::File::create(format!("{outdir}/frame_{frame_idx}.png")).unwrap();
        frame_idx += 1;
        let mut encoder = png::Encoder::new(file, pw, ph);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&buffer).unwrap();
        writer.finish().unwrap();
        ball_x(&plan)
    };

    let x_init = render("initial (settled at t=0)", 0.0);

    // Scrub to t=50% (the ball's peak: left 0 -> 280px).
    store::dispatch(&state_path, "time:5");
    let x0 = render("transition start", 0.0);
    let x_mid = render("transition mid (pre-rendered +0.75s)", 0.75);
    let x_end = render("transition end (pre-rendered +1.6s)", 1.6);
    assert!(x0 < x_mid && x_mid < x_end, "should ease forward");
    assert!(
        (x_end - x_init - 280.0).abs() < 1.0,
        "should settle at peak, got {x_end}"
    );

    // Let the transition to the peak finish, head back home, then interrupt
    // mid-flight by re-targeting the peak.
    std::thread::sleep(std::time::Duration::from_secs_f64(
        demo::TRANSITION_SECS + 0.2,
    ));
    store::dispatch(&state_path, "time:0");
    std::thread::sleep(std::time::Duration::from_secs_f64(0.5));
    store::dispatch(&state_path, "time:5");
    let x_int = render("interrupted: re-baselined from mid-flight", 0.0);
    let x_settled = render("settled back at peak (pre-rendered +1.6s)", 1.6);
    assert!(
        x_int > x_init + 20.0 && x_int < x_init + 260.0,
        "interrupt should start mid-flight, got {x_int}"
    );
    assert!(
        (x_settled - x_init - 280.0).abs() < 1.0,
        "should settle back at peak, got {x_settled}"
    );

    let state = store::load(&state_path);
    println!(
        "state: anim_time={} trans_from={} (duration {}s)",
        state.anim_time,
        state.trans_from,
        demo::TRANSITION_SECS
    );
    println!("Interrupted transition eased from mid-flight. Wrote frames to {outdir}/");
}
