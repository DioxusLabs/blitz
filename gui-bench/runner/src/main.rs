//! `runner run [--apps a,b] [--workloads table,scroll] [--out-dir DIR] [-- app args...]`
//!     Builds each app in release mode, runs each workload, writes `DIR/<app>-<workload>.jsonl`.
//! `runner report FILE...`
//!     Prints Markdown tables of per-phase medians and p95s (and frame-time p50/p95/p99
//!     for scroll) from the JSON lines files.

use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const APPS: &[&str] = &["dioxus-native", "iced", "egui", "slint", "gpui", "freya"];
const WORKLOADS: &[&str] = &["table", "scroll"];
const PHASES: &[&str] = &[
    "update_us",
    "build_us",
    "raster_cpu_us",
    "raster_gpu_us",
    "acquire_us",
    "present_us",
    "total_us",
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("run") => run(&args[1..]),
        Some("report") => {
            let files: Vec<PathBuf> = args[1..].iter().map(PathBuf::from).collect();
            print!("{}", report(&files));
        }
        _ => {
            eprintln!(
                "usage: runner run [--apps a,b] [--workloads table,scroll] [--out-dir DIR] [-- app args...]"
            );
            eprintln!("       runner report FILE...");
            std::process::exit(2);
        }
    }
}

fn list(value: &str) -> Vec<String> {
    value.split(',').map(str::to_owned).collect()
}

fn run(args: &[String]) {
    let mut apps: Vec<String> = APPS.iter().map(|s| s.to_string()).collect();
    let mut workloads: Vec<String> = WORKLOADS.iter().map(|s| s.to_string()).collect();
    let mut out_dir = PathBuf::from("results");
    let mut app_args = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--apps" => apps = list(it.next().expect("--apps value")),
            "--workloads" => workloads = list(it.next().expect("--workloads value")),
            "--out-dir" => out_dir = it.next().expect("--out-dir value").into(),
            "--" => app_args.extend(it.by_ref().cloned()),
            other => panic!("unknown argument {other:?}"),
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    std::fs::create_dir_all(&out_dir).unwrap();
    let mut files = Vec::new();
    for app in &apps {
        let manifest = root.join("apps").join(app).join("Cargo.toml");
        let status = Command::new("cargo")
            .args(["build", "--release", "--manifest-path"])
            .arg(&manifest)
            .status()
            .unwrap();
        if !status.success() {
            eprintln!("runner: {app} failed to build, skipping");
            continue;
        }
        let exe = root
            .join("apps")
            .join(app)
            .join("target/release")
            .join(format!("bench-{app}"));
        for workload in &workloads {
            let out = out_dir.join(format!("{app}-{workload}.jsonl"));
            eprintln!("runner: {app} {workload} -> {}", out.display());
            let status = Command::new(&exe)
                .args(["--workload", workload, "--out"])
                .arg(&out)
                .args(&app_args)
                .status()
                .unwrap();
            if status.success() {
                files.push(out);
            } else {
                eprintln!("runner: {app} {workload} exited with {status}");
            }
        }
    }
    print!("{}", report(&files));
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let rank = (p / 100.0 * (sorted.len() - 1) as f64).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

#[derive(Default)]
struct Series {
    meta: BTreeMap<String, String>,
    /// bench -> phase -> samples
    samples: BTreeMap<String, BTreeMap<String, Vec<f64>>>,
}

fn load(files: &[PathBuf]) -> BTreeMap<(String, String), Series> {
    let mut series: BTreeMap<(String, String), Series> = BTreeMap::new();
    for file in files {
        let text =
            std::fs::read_to_string(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let v: Value = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("{}: {e}: {line}", file.display()));
            let key = (
                v["workload"].as_str().unwrap_or_default().to_owned(),
                v["toolkit"].as_str().unwrap_or_default().to_owned(),
            );
            let s = series.entry(key).or_default();
            match v["type"].as_str() {
                Some("meta") => {
                    for (k, val) in v.as_object().unwrap() {
                        if let Some(val) = val.as_str() {
                            s.meta.insert(k.clone(), val.to_owned());
                        }
                    }
                }
                Some("frame") => {
                    let bench = v["bench"].as_str().unwrap_or_default().to_owned();
                    let phases = s.samples.entry(bench).or_default();
                    for phase in PHASES {
                        if let Some(us) = v[*phase].as_f64() {
                            phases.entry(phase.to_string()).or_default().push(us);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    series
}

fn fmt_ms(us: f64) -> String {
    format!("{:.2}", us / 1000.0)
}

fn report(files: &[PathBuf]) -> String {
    let mut out = String::new();
    let series = load(files);
    if series.is_empty() {
        return out;
    }
    out.push_str(
        "## Runs\n\n| workload | toolkit | renderer | adapter | os |\n|---|---|---|---|---|\n",
    );
    for ((workload, toolkit), s) in &series {
        let m = |k: &str| s.meta.get(k).cloned().unwrap_or_else(|| "-".into());
        out.push_str(&format!(
            "| {workload} | {toolkit} | {} | {} | {} |\n",
            m("renderer"),
            m("adapter"),
            m("os")
        ));
    }
    let mut benches: Vec<(String, String)> = Vec::new();
    for ((workload, _), s) in &series {
        for bench in s.samples.keys() {
            let key = (workload.clone(), bench.clone());
            if !benches.contains(&key) {
                benches.push(key);
            }
        }
    }
    for (workload, bench) in benches {
        out.push_str(&format!(
            "\n## {workload}: {bench}\n\nMedian (p95) ms per phase; `-` means the toolkit doesn't expose that phase.\n\n| toolkit | n |"
        ));
        for phase in PHASES {
            out.push_str(&format!(" {} |", phase.trim_end_matches("_us")));
        }
        if workload == "scroll" {
            out.push_str(" total p99 | frames > 16.7ms |");
        }
        out.push_str("\n|---|---|");
        for _ in PHASES {
            out.push_str("---|");
        }
        if workload == "scroll" {
            out.push_str("---|---|");
        }
        out.push('\n');
        for ((w, toolkit), s) in &series {
            if *w != workload {
                continue;
            }
            let Some(phases) = s.samples.get(&bench) else {
                continue;
            };
            let n = phases.get("total_us").map_or(0, Vec::len);
            out.push_str(&format!("| {toolkit} | {n} |"));
            let mut total_sorted = Vec::new();
            for phase in PHASES {
                match phases.get(*phase) {
                    Some(samples) if !samples.is_empty() => {
                        let mut sorted = samples.clone();
                        sorted.sort_by(f64::total_cmp);
                        out.push_str(&format!(
                            " {} ({}) |",
                            fmt_ms(percentile(&sorted, 50.0)),
                            fmt_ms(percentile(&sorted, 95.0))
                        ));
                        if *phase == "total_us" {
                            total_sorted = sorted;
                        }
                    }
                    _ => out.push_str(" - |"),
                }
            }
            if workload == "scroll" && !total_sorted.is_empty() {
                let slow = total_sorted.iter().filter(|&&us| us > 16_667.0).count();
                out.push_str(&format!(
                    " {} | {slow} |",
                    fmt_ms(percentile(&total_sorted, 99.0))
                ));
            }
            out.push('\n');
        }
    }
    out
}
