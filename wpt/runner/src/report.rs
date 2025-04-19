//! Code related to writing a report in "WPT Report" format

use std::{path::Path, process::Command};
use wptreport::{
    reports::wpt_report::{self, WptRunInfo},
    wpt_report::WptReport,
};

use crate::{TestResult, TestStatus};

fn get_git_hash(path: &Path) -> String {
    let output = Command::new("git")
        .current_dir(path)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("Failed to run git rev-parse HEAD");
    if !output.status.success() {
        panic!("Failed to run git rev-parse HEAD (command failed)")
    }
    let hash = String::from_utf8(output.stdout)
        .expect("Failed to run git rev-parse HEAD (non-utf8 output)");
    // Remove trailing newline
    hash.trim().to_string()
}

pub fn generate_run_info(wpt_dir: &Path) -> WptRunInfo {
    let os_info = os_info::get();

    WptRunInfo {
        product: String::from("blitz"),
        revision: get_git_hash(wpt_dir),
        browser_version: Some(get_git_hash(&std::env::current_dir().unwrap())),
        automation: true,
        debug: cfg!(debug_assertions),
        display: None,
        has_sandbox: false,
        headless: true,
        verify: false,
        wasm: false,
        os: String::new(),
        os_version: String::new(),
        version: String::new(),
        processor: String::new(),
        bits: match os_info.bitness() {
            os_info::Bitness::X32 => 32,
            os_info::Bitness::X64 => 64,
            os_info::Bitness::Unknown | _ => 0,
        },
        python_version: 0,
        apple_catalina: false,
        apple_silicon: false,
        win10_2004: false,
        win10_2009: false,
        win11_2009: false,
    }
}

fn convert_status(status: TestStatus) -> wpt_report::TestStatus {
    match status {
        TestStatus::Pass => wpt_report::TestStatus::Pass,
        TestStatus::Fail => wpt_report::TestStatus::Fail,
        TestStatus::Skip => wpt_report::TestStatus::Skip,
        TestStatus::Crash => wpt_report::TestStatus::Crash,
    }
}

fn convert_subtest_status(status: TestStatus) -> wpt_report::SubtestStatus {
    match status {
        TestStatus::Pass => wpt_report::SubtestStatus::Pass,
        TestStatus::Fail => wpt_report::SubtestStatus::Fail,
        TestStatus::Skip => wpt_report::SubtestStatus::Skip,
        TestStatus::Crash => unreachable!(),
    }
}

pub fn generate_report(
    wpt_dir: &Path,
    results: Vec<TestResult>,
    time_start: u64,
    time_end: u64,
) -> WptReport {
    let results: Vec<_> = results
        .into_iter()
        .map(|test| wpt_report::TestResult {
            test: test.name,
            status: convert_status(test.status),
            duration: test.duration.as_millis() as i64,
            message: test.panic_info.and_then(|info| info.message),
            known_intermittent: Vec::new(),
            subsuite: String::new(),
            subtests: test
                .subtest_results
                .into_iter()
                .map(|subtest| wpt_report::SubtestResult {
                    name: subtest.name,
                    status: convert_subtest_status(subtest.status),
                    message: if subtest.errors.is_empty() {
                        None
                    } else {
                        Some(subtest.errors.join("\n"))
                    },
                    known_intermittent: Vec::new(),
                })
                .collect(),
        })
        .collect();

    WptReport {
        time_start,
        time_end,
        run_info: generate_run_info(wpt_dir),
        results,
    }
}

pub fn generate_expectations(results: &[TestResult]) -> String {
    let mut out = String::with_capacity(10 * 1024 * 1024); // 10MB

    for test in results {
        out.push_str(&test.name);
        out.push(' ');
        out.push_str(test.status.as_str());
        out.push(' ');

        for subtest in &test.subtest_results {
            let c = match subtest.status {
                TestStatus::Pass => 'Y',
                TestStatus::Fail => 'N',
                TestStatus::Skip => '.',
                TestStatus::Crash => unreachable!(),
            };
            out.push(c);
        }

        out.push('\n');
    }

    out
}
