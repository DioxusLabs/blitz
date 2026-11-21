use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Workload {
    Table,
    Scroll,
}

impl Workload {
    pub fn name(self) -> &'static str {
        match self {
            Workload::Table => "table",
            Workload::Scroll => "scroll",
        }
    }
}

/// Command line shared by every app:
/// `<app> [--workload table|scroll] [--iters N] [--warmup N] [--frames N] [--out FILE]
///        [--no-gpu-timing] [--vsync]`
#[derive(Clone, Debug)]
pub struct Args {
    pub workload: Workload,
    /// Timed iterations of each table benchmark.
    pub iters: usize,
    /// Untimed iterations (table) or frames (scroll) run first.
    pub warmup: usize,
    /// Timed scroll frames.
    pub frames: usize,
    /// JSON lines output; stdout when unset.
    pub out: Option<PathBuf>,
    /// Skip GPU timestamp queries even when the adapter supports them.
    pub gpu_timing: bool,
    /// Present with vsync (frame pacing runs). Off by default (throughput runs).
    pub vsync: bool,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            workload: Workload::Table,
            iters: 10,
            warmup: 3,
            frames: 600,
            out: None,
            gpu_timing: true,
            vsync: false,
        }
    }
}

impl Args {
    pub fn parse() -> Self {
        Self::parse_from(std::env::args().skip(1))
    }

    pub fn parse_from(args: impl IntoIterator<Item = String>) -> Self {
        let mut out = Args::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let mut value = |name: &str| {
                args.next()
                    .unwrap_or_else(|| panic!("{name} needs a value"))
            };
            match arg.as_str() {
                "--workload" => {
                    out.workload = match value("--workload").as_str() {
                        "table" => Workload::Table,
                        "scroll" => Workload::Scroll,
                        other => panic!("unknown workload {other:?} (expected table or scroll)"),
                    }
                }
                "--iters" => out.iters = value("--iters").parse().expect("--iters"),
                "--warmup" => out.warmup = value("--warmup").parse().expect("--warmup"),
                "--frames" => out.frames = value("--frames").parse().expect("--frames"),
                "--out" => out.out = Some(value("--out").into()),
                "--no-gpu-timing" => out.gpu_timing = false,
                "--vsync" => out.vsync = true,
                other => panic!("unknown argument {other:?}"),
            }
        }
        out
    }
}
