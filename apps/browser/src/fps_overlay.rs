use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dioxus_native::prelude::*;
use dioxus_native::{use_wgpu, CustomPaintCtx, CustomPaintSource, DeviceHandle, TextureHandle};

const RING_LEN: usize = 60;

struct FpsStats {
    last: Option<Instant>,
    ring: [Duration; RING_LEN],
    idx: usize,
    count: usize,
}

impl Default for FpsStats {
    fn default() -> Self {
        Self {
            last: None,
            ring: [Duration::ZERO; RING_LEN],
            idx: 0,
            count: 0,
        }
    }
}

impl FpsStats {
    fn record(&mut self, now: Instant) {
        if let Some(prev) = self.last {
            let dt = now.duration_since(prev);
            self.ring[self.idx] = dt;
            self.idx = (self.idx + 1) % RING_LEN;
            if self.count < RING_LEN {
                self.count += 1;
            }
        }
        self.last = Some(now);
    }

    fn snapshot(&self) -> (f32, f32) {
        if self.count == 0 {
            return (0.0, 0.0);
        }
        let total: Duration = self.ring[..self.count].iter().copied().sum();
        let avg = total / self.count as u32;
        let ms = avg.as_secs_f32() * 1000.0;
        let fps = if ms > 0.0 { 1000.0 / ms } else { 0.0 };
        (fps, ms)
    }

    fn reset(&mut self) {
        self.last = None;
        self.count = 0;
        self.idx = 0;
    }
}

struct FpsPaintSource {
    stats: Arc<Mutex<FpsStats>>,
}

impl CustomPaintSource for FpsPaintSource {
    fn resume(&mut self, _device_handle: &DeviceHandle) {}

    fn suspend(&mut self) {
        if let Ok(mut s) = self.stats.lock() {
            s.reset();
        }
    }

    fn render(
        &mut self,
        _ctx: CustomPaintCtx<'_>,
        _width: u32,
        _height: u32,
        _scale: f64,
    ) -> Option<TextureHandle> {
        if let Ok(mut s) = self.stats.lock() {
            s.record(Instant::now());
        }
        None
    }
}

#[component]
pub fn FpsOverlay() -> Element {
    let stats: Arc<Mutex<FpsStats>> = use_hook(|| Arc::new(Mutex::new(FpsStats::default())));

    let stats_for_source = stats.clone();
    let paint_source_id = use_wgpu(move || FpsPaintSource {
        stats: stats_for_source,
    });

    let mut display = use_signal(|| (0.0_f32, 0.0_f32));

    let stats_for_poll = stats.clone();
    use_hook(move || {
        spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(250)).await;
                let snap = stats_for_poll
                    .lock()
                    .map(|s| s.snapshot())
                    .unwrap_or((0.0, 0.0));
                display.set(snap);
            }
        });
    });

    let (fps, ms) = display();

    rsx!(
        canvas { class: "fps-tick", "src": paint_source_id }
        div { class: "fps-overlay", "{fps:.0} FPS / {ms:.1} ms" }
    )
}
