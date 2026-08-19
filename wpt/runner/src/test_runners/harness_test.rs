//! Runner for testharness.js tests: executes the test file's JavaScript (including
//! the real testharness.js framework) using `blitz-script`, and collects the
//! harness results via a custom `testharnessreport.js`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use blitz_dom::Document as _;
use blitz_script::{FetchError, ScriptDocument, ScriptFetcher};
use log::warn;
use url::Url;

use super::{SubtestResult, parse_and_resolve_document};
use crate::{SubtestCounts, TestStatus, ThreadCtx};

/// How long to wait for the harness to complete (async tests, timers)
const HARNESS_TIMEOUT: Duration = Duration::from_secs(5);

/// Custom `testharnessreport.js` served in place of WPT's stock one (which is
/// designed to be replaced by test runners). It disables the DOM output section
/// and reports results to the runner via the `__blitz_send_message` native
/// function provided by blitz-script.
///
/// The `timeout_multiplier` scales down testharness's internal timeout (10s by
/// default) so that tests which will never complete (e.g. ones waiting for
/// events that Blitz never fires, or for testdriver.js automation) report a
/// TIMEOUT harness status quickly instead of hitting the runner's
/// [`HARNESS_TIMEOUT`] backstop. It also proportionally shortens
/// `step_timeout()` delays, speeding up legitimately-passing async tests.
const TESTHARNESSREPORT_JS: &str = r#"
setup({ output: false, debug: false, timeout_multiplier: 0.1 });
add_completion_callback(function (tests, harness_status) {
    __blitz_send_message(JSON.stringify({
        type: "wpt_results",
        harness_status: {
            status: harness_status.status,
            message: harness_status.message,
        },
        tests: tests.map(function (test) {
            return { name: test.name, status: test.status, message: test.message };
        }),
    }));
});
"#;

/// Custom `testdriver-vendor.js` served in place of WPT's stock one (an empty
/// file which automation environments are expected to replace).
///
/// Blitz has no testdriver automation backend. Without a vendor file,
/// testdriver.js commands like `test_driver.click()` fall back to waiting for
/// a *real user* to perform the action, so such tests hang until the harness
/// timeout. Setting `in_automation` makes every command reject immediately
/// ("...not implemented by testdriver-vendor.js" errors from testdriver.js's
/// default `test_driver_internal` implementations), converting those timeouts
/// into fast failures.
const TESTDRIVER_VENDOR_JS: &str = r#"
window.test_driver_internal.in_automation = true;
"#;

/// A [`ScriptFetcher`] which resolves script URLs against the local WPT checkout,
/// and intercepts `/resources/testharnessreport.js` and
/// `/resources/testdriver-vendor.js` to serve the custom versions above.
pub struct WptScriptFetcher {
    wpt_dir: PathBuf,
}

impl WptScriptFetcher {
    pub fn new(wpt_dir: PathBuf) -> Self {
        Self { wpt_dir }
    }
}

impl ScriptFetcher for WptScriptFetcher {
    fn fetch(&self, url: &Url) -> Result<String, FetchError> {
        let path = url.path();
        if path.ends_with("/resources/testharnessreport.js") {
            return Ok(TESTHARNESSREPORT_JS.to_string());
        }
        if path.ends_with("/resources/testdriver-vendor.js") {
            return Ok(TESTDRIVER_VENDOR_JS.to_string());
        }
        let relative_path = path.strip_prefix('/').unwrap_or(path);
        std::fs::read_to_string(self.wpt_dir.join(relative_path)).map_err(FetchError::Io)
    }
}

pub fn process_harness_test(
    ctx: &mut ThreadCtx,
    html: &str,
    relative_path: &str,
) -> (TestStatus, SubtestCounts, Vec<SubtestResult>) {
    let document = parse_and_resolve_document(ctx, html, relative_path);
    let mut document = ScriptDocument::from_base_document(document)
        .with_fetcher(WptScriptFetcher::new(ctx.wpt_dir.clone()));

    document.execute_scripts();

    // Pump timers until the harness reports results (or the timeout expires).
    // Synchronous tests complete during `execute_scripts` (testharness completes
    // on the `load` event); async tests may schedule timers.
    //
    // Uncaught JS errors fail the test immediately: in a real browser they would
    // reach testharness's window `error` handler and produce a harness ERROR, but
    // here they typically mean the harness will never complete.
    let deadline = Instant::now() + HARNESS_TIMEOUT;
    let results = loop {
        let messages = document.take_messages();
        if let Some(results) = messages.iter().find_map(|msg| parse_results(msg)) {
            break results;
        }

        let js_errors = document.take_js_errors();
        if !js_errors.is_empty() {
            for error in &js_errors {
                warn!("{relative_path}: {error}");
            }
            let subtest_results = vec![SubtestResult {
                name: "Uncaught JS error".to_string(),
                status: TestStatus::Fail,
                errors: js_errors,
            }];
            return (
                TestStatus::Fail,
                SubtestCounts::ZERO_OF_ONE,
                subtest_results,
            );
        }

        let now = Instant::now();
        let has_expired = now >= deadline;
        let next_timer_deadline = document.next_timer_deadline();
        let (false, Some(timer_deadline)) = (has_expired, next_timer_deadline) else {
            // Timeout, or no pending timers: the harness will never complete
            warn!("{relative_path}: testharness.js did not report results");
            return (TestStatus::Fail, SubtestCounts::ZERO_OF_ONE, Vec::new());
        };

        let wake_at = timer_deadline.min(deadline);
        if wake_at > now {
            std::thread::sleep(wake_at - now);
        }
        document.poll(None);
    };

    let (harness_status, subtest_results) = results;
    harness_outcome(harness_status, subtest_results)
}

/// Compute the overall test outcome from a testharness.js harness status code
/// and its subtest results
pub(super) fn harness_outcome(
    harness_status: i64,
    subtest_results: Vec<SubtestResult>,
) -> (TestStatus, SubtestCounts, Vec<SubtestResult>) {
    // A non-OK harness status (ERROR/TIMEOUT) fails the test even if subtests passed
    if harness_status != 0 && subtest_results.is_empty() {
        return (TestStatus::Fail, SubtestCounts::ZERO_OF_ONE, Vec::new());
    }

    let pass_count = subtest_results
        .iter()
        .filter(|result| matches!(result.status, TestStatus::Pass))
        .count() as u32;
    let subtest_counts = SubtestCounts {
        pass: pass_count,
        total: subtest_results.len() as u32,
    };
    let status = if harness_status != 0 {
        TestStatus::Fail
    } else {
        subtest_counts.as_status()
    };

    (status, subtest_counts, subtest_results)
}

/// Parse the JSON results message sent by the custom `testharnessreport.js`.
/// Returns the harness status code and the subtest results.
pub(super) fn parse_results(message: &str) -> Option<(i64, Vec<SubtestResult>)> {
    let value: serde_json::Value = serde_json::from_str(message).ok()?;
    if value.get("type")?.as_str()? != "wpt_results" {
        return None;
    }

    let harness_status = value.get("harness_status")?.get("status")?.as_i64()?;
    let subtest_results = value
        .get("tests")?
        .as_array()?
        .iter()
        .map(|test| {
            let name = test
                .get("name")
                .and_then(|name| name.as_str())
                .unwrap_or("")
                .to_string();
            // Test statuses: 0 = PASS, 1 = FAIL, 2 = TIMEOUT, 3 = NOTRUN, 4 = PRECONDITION_FAILED
            let status_code = test.get("status").and_then(|s| s.as_i64()).unwrap_or(1);
            let status = if status_code == 0 {
                TestStatus::Pass
            } else {
                TestStatus::Fail
            };
            let errors = test
                .get("message")
                .and_then(|msg| msg.as_str())
                .map(|msg| vec![msg.to_string()])
                .unwrap_or_default();
            SubtestResult {
                name,
                status,
                errors,
            }
        })
        .collect();

    Some((harness_status, subtest_results))
}
