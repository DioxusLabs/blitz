//! Diffing a local test run against the latest results from the `main` branch.
//!
//! The `main` branch results are published to GitHub Pages by the WPT CI workflow. They are cached
//! locally (in `wpt/cache`) and revalidated with a conditional (`If-Modified-Since`) request on
//! each run.

use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::time::Duration;

use owo_colors::OwoColorize;
use reqwest::StatusCode;
use reqwest::blocking::Client;
use wptreport::TestResultIter as _;
use wptreport::aggregate::aggregate;
use wptreport::wpt_report::{TestResult, TestStatus, WptReport};

/// The `main` branch report, as published by the WPT CI workflow
const MAIN_REPORT_URL: &str = "https://dioxuslabs.github.io/blitz/wptreport.json.zst";
const REPORT_FILE_NAME: &str = "main-wptreport.json.zst";
const LAST_MODIFIED_FILE_NAME: &str = "main-wptreport.last-modified";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Download the latest `main` branch report, using (and updating) a local cache in `cache_dir`.
///
/// The report is filtered down to the tests belonging to `suites` so that a partial run is only
/// diffed against the corresponding part of the `main` branch results.
///
/// Returns `None` if the report could neither be downloaded nor read from the cache.
pub fn fetch_main_report(cache_dir: &Path, suites: &[String]) -> Option<WptReport> {
    let report_path = cache_dir.join(REPORT_FILE_NAME);
    let last_modified_path = cache_dir.join(LAST_MODIFIED_FILE_NAME);

    if let Err(err) = fs::create_dir_all(cache_dir) {
        eprintln!("Failed to create cache directory {cache_dir:?}: {err}");
        return None;
    }

    // Only send If-Modified-Since if we actually have the corresponding cached report
    let cached_last_modified = report_path
        .exists()
        .then(|| fs::read_to_string(&last_modified_path).ok())
        .flatten();

    match download(cached_last_modified.as_deref()) {
        Ok(Some(response)) => {
            if let Err(err) = fs::write(&report_path, &response.body) {
                eprintln!("Failed to write cached report to {report_path:?}: {err}");
                return None;
            }
            match response.last_modified {
                Some(last_modified) => {
                    let _ = fs::write(&last_modified_path, last_modified);
                }
                // Without a Last-Modified value we cannot revalidate: drop any stale one
                None => {
                    let _ = fs::remove_file(&last_modified_path);
                }
            }
            println!("Downloaded new main-branch results");
        }
        Ok(None) => println!("Cached main-branch results are up to date"),
        Err(err) => {
            eprintln!("Failed to download main-branch results: {err}");
            if !report_path.exists() {
                return None;
            }
            eprintln!("Falling back to cached results");
        }
    }

    let mut report = parse_report(&report_path)?;
    report
        .results
        .retain(|test| suites.iter().any(|suite| test.test.starts_with(suite)));

    Some(report)
}

struct Response {
    body: Vec<u8>,
    last_modified: Option<String>,
}

/// Returns `Ok(None)` if the server responded that the cached copy is still up to date.
fn download(cached_last_modified: Option<&str>) -> Result<Option<Response>, String> {
    let client = Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|err| err.to_string())?;

    let mut request = client.get(MAIN_REPORT_URL);
    if let Some(last_modified) = cached_last_modified {
        request = request.header("If-Modified-Since", last_modified);
    }

    let response = request.send().map_err(|err| err.to_string())?;

    if response.status() == StatusCode::NOT_MODIFIED {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(format!("{MAIN_REPORT_URL} returned {}", response.status()));
    }

    let last_modified = response
        .headers()
        .get("last-modified")
        .and_then(|value| value.to_str().ok())
        .map(String::from);
    let body = response.bytes().map_err(|err| err.to_string())?.to_vec();

    Ok(Some(Response {
        body,
        last_modified,
    }))
}

fn parse_report(report_path: &Path) -> Option<WptReport> {
    let compressed = fs::read(report_path)
        .map_err(|err| format!("Failed to read {report_path:?}: {err}"))
        .ok()?;
    let json = zstd::decode_all(Cursor::new(&compressed))
        .map_err(|err| format!("Failed to decompress {report_path:?}: {err}"))
        .ok()?;
    serde_json::from_slice(&json)
        .map_err(|err| format!("Failed to parse {report_path:?}: {err}"))
        .ok()
}

/// Print a diff of `current` against `main_report` (the baseline).
pub fn print_diff(main_report: WptReport, current: WptReport) {
    println!(
        "\nDiff against main ({})\n=====================",
        main_report
            .run_info
            .browser_version
            .as_deref()
            .unwrap_or("unknown revision")
    );

    let mut added = 0u32;
    let mut removed = 0u32;
    let mut improved = 0u32;
    let mut regressed = 0u32;

    let mut reports = [main_report, current];
    aggregate(&mut reports, |results| match (results[0], results[1]) {
        (None, None) => unreachable!(),
        (Some(base), None) => {
            removed += 1;
            println!("{} {}", "REM ".bright_black(), base.test);
        }
        (None, Some(new)) => {
            added += 1;
            println!("{} {}", "ADD ".bright_black(), new.test);
        }
        (Some(base), Some(new)) => {
            let base_counts = base.subtest_counts();
            let new_counts = new.subtest_counts();
            if base_counts == new_counts {
                return;
            }

            let change = format!(
                "{} => {} {}",
                format_counts(base),
                format_counts(new),
                new.test
            );
            if new_counts.pass_fraction() >= base_counts.pass_fraction() {
                improved += 1;
                println!("{}", change.green());
            } else {
                regressed += 1;
                println!("{}", change.red());
            }
        }
    });

    println!("---\n");
    println!("{regressed:>4} tests REGRESSED");
    println!("{improved:>4} tests IMPROVED");
    println!("{added:>4} tests ADDED");
    println!("{removed:>4} tests REMOVED");
    if regressed == 0 && improved == 0 && added == 0 && removed == 0 {
        println!("\nNo changes compared to main");
    }
}

fn format_counts(test: &TestResult) -> String {
    let counts = test.subtest_counts();
    let status = match test.status {
        TestStatus::Pass | TestStatus::Ok => "PASS",
        TestStatus::Fail => "FAIL",
        TestStatus::Skip => "SKIP",
        TestStatus::Crash => "CRASH",
        TestStatus::Error => "ERROR",
        TestStatus::Timeout => "TIMEOUT",
        TestStatus::Assert => "ASSERT",
        TestStatus::PreconditionFailed => "PRECONDITION_FAILED",
    };
    format!("{status}({}/{})", counts.pass, counts.total)
}
