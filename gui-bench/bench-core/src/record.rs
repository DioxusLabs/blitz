use crate::scroll::SCROLL_STEP;
use crate::table::BENCHES;
use crate::{Args, Op, Workload};
use std::fmt::Write as _;
use std::io::Write;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Update,
    Build,
    RasterCpu,
    RasterGpu,
    Acquire,
    Present,
}

impl Phase {
    pub const ALL: [Phase; 6] = [
        Phase::Update,
        Phase::Build,
        Phase::RasterCpu,
        Phase::RasterGpu,
        Phase::Acquire,
        Phase::Present,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Phase::Update => "update_us",
            Phase::Build => "build_us",
            Phase::RasterCpu => "raster_cpu_us",
            Phase::RasterGpu => "raster_gpu_us",
            Phase::Acquire => "acquire_us",
            Phase::Present => "present_us",
        }
    }
}

struct Step {
    op: Op,
    /// Benchmark name and iteration for timed steps; `None` for setup and warmup.
    timed: Option<(&'static str, usize)>,
}

fn table_script(args: &Args) -> Vec<Step> {
    let mut steps = Vec::new();
    for bench in BENCHES {
        for iter in 0..args.warmup + args.iters {
            for &op in bench.setup {
                steps.push(Step {
                    op: Op::Table(op),
                    timed: None,
                });
            }
            let timed = (iter >= args.warmup).then(|| (bench.name, iter - args.warmup));
            steps.push(Step {
                op: Op::Table(bench.op),
                timed,
            });
        }
    }
    steps
}

fn scroll_script(args: &Args) -> Vec<Step> {
    (0..args.warmup + args.frames)
        .map(|frame| Step {
            op: Op::Scroll {
                y: frame as f32 * SCROLL_STEP,
            },
            timed: (frame >= args.warmup).then(|| ("scroll", frame - args.warmup)),
        })
        .collect()
}

/// Feeds an app one op per frame and records the frame's phase timings as JSON lines.
///
/// Per frame: `begin_frame` (returns the op and starts the clock), then `lap` after each
/// phase (or `add` for phases measured elsewhere, such as GPU time), then `end_frame`.
pub struct Driver {
    toolkit: &'static str,
    workload: Workload,
    steps: Vec<Step>,
    pos: usize,
    frame_start: Option<Instant>,
    presented_us: Option<f64>,
    last_lap: Instant,
    phases: [Option<f64>; 6],
    out: Box<dyn Write>,
}

impl Driver {
    pub fn new(toolkit: &'static str, args: &Args) -> Self {
        let steps = match args.workload {
            Workload::Table => table_script(args),
            Workload::Scroll => scroll_script(args),
        };
        let out: Box<dyn Write> = match &args.out {
            Some(path) => Box::new(std::io::BufWriter::new(
                std::fs::File::create(path).expect("create --out file"),
            )),
            None => Box::new(std::io::stdout()),
        };
        Driver {
            toolkit,
            workload: args.workload,
            steps,
            pos: 0,
            frame_start: None,
            presented_us: None,
            last_lap: Instant::now(),
            phases: [None; 6],
            out,
        }
    }

    /// Write the run's metadata line (renderer, adapter, scale factor, versions, ...).
    pub fn meta(&mut self, fields: &[(&str, String)]) {
        let mut line = format!(
            r#"{{"type":"meta","toolkit":"{}","workload":"{}","os":"{}""#,
            self.toolkit,
            self.workload.name(),
            std::env::consts::OS
        );
        for (key, value) in fields {
            write!(line, r#","{}":"{}""#, key, escape(value)).unwrap();
        }
        line.push('}');
        writeln!(self.out, "{line}").unwrap();
    }

    pub fn is_done(&self) -> bool {
        self.pos >= self.steps.len()
    }

    /// The op for this frame, or `None` once the script is finished (the app should exit).
    pub fn begin_frame(&mut self) -> Option<Op> {
        self.begin_frame_at(Instant::now())
    }

    /// Like [`Self::begin_frame`], for a frame that started earlier (apps that must
    /// acquire a swapchain image before they can commit to rendering the op).
    pub fn begin_frame_at(&mut self, now: Instant) -> Option<Op> {
        let step = self.steps.get(self.pos)?;
        self.frame_start = Some(now);
        self.last_lap = now;
        self.phases = [None; 6];
        self.presented_us = None;
        Some(step.op.clone())
    }

    /// Whether a frame is in progress (between `begin_frame` and `end_frame`).
    pub fn in_frame(&self) -> bool {
        self.frame_start.is_some()
    }

    /// Attribute the time since the previous lap (or `begin_frame`) to `phase`.
    pub fn lap(&mut self, phase: Phase) {
        let now = Instant::now();
        let us = (now - self.last_lap).as_secs_f64() * 1e6;
        self.last_lap = now;
        self.add(phase, us);
    }

    /// Skip the time since the previous lap (work that belongs to no phase).
    pub fn skip(&mut self) {
        self.last_lap = Instant::now();
    }

    /// Add `us` microseconds to `phase`.
    pub fn add(&mut self, phase: Phase, us: f64) {
        let slot = &mut self.phases[phase as usize];
        *slot = Some(slot.unwrap_or(0.0) + us);
    }

    /// Fix `total` (op injection until now) without ending the frame, so work after
    /// present (waiting for GPU timestamps) isn't counted.
    pub fn presented(&mut self) {
        if let Some(start) = self.frame_start {
            self.presented_us = Some(start.elapsed().as_secs_f64() * 1e6);
        }
    }

    /// Finish the frame. `total` is op injection until [`Self::presented`], or until now.
    pub fn end_frame(&mut self) {
        let Some(start) = self.frame_start.take() else {
            return;
        };
        let total = self
            .presented_us
            .take()
            .unwrap_or_else(|| start.elapsed().as_secs_f64() * 1e6);
        let step = &self.steps[self.pos];
        self.pos += 1;
        let Some((bench, iter)) = step.timed else {
            return;
        };
        let mut line = format!(
            r#"{{"type":"frame","toolkit":"{}","workload":"{}","bench":"{bench}","iter":{iter},"total_us":{total:.1}"#,
            self.toolkit,
            self.workload.name(),
        );
        for phase in Phase::ALL {
            if let Some(us) = self.phases[phase as usize] {
                write!(line, r#","{}":{us:.1}"#, phase.name()).unwrap();
            }
        }
        line.push('}');
        writeln!(self.out, "{line}").unwrap();
        if self.is_done() {
            self.out.flush().unwrap();
        }
    }
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_script_times_only_the_benchmark_op() {
        let args = Args {
            iters: 2,
            warmup: 1,
            ..Args::default()
        };
        let steps = table_script(&args);
        let timed = steps.iter().filter(|s| s.timed.is_some()).count();
        assert_eq!(timed, BENCHES.len() * 2);
    }

    #[test]
    fn scroll_script_advances_at_a_fixed_rate() {
        let args = Args {
            workload: Workload::Scroll,
            frames: 5,
            warmup: 2,
            ..Args::default()
        };
        let steps = scroll_script(&args);
        assert_eq!(steps.len(), 7);
        assert_eq!(
            steps[6].op,
            Op::Scroll {
                y: 6.0 * SCROLL_STEP
            }
        );
        assert_eq!(steps[2].timed, Some(("scroll", 0)));
    }
}
