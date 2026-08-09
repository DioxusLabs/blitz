use std::{fs, sync::Arc, time::Instant};

use blitz_dom::{BaseDocument, DocumentConfig};
use blitz_html::HtmlDocument;
use log::{debug, warn};

use crate::{SubtestCounts, TestFlags, TestKind, TestStatus, ThreadCtx};

mod attr_test;
mod crash_test;
mod ref_test;

pub use attr_test::process_attr_test;
pub use crash_test::process_crash_test;
pub use ref_test::process_ref_test;

pub struct SubtestResult {
    pub name: String,
    pub status: TestStatus,
    pub errors: Vec<String>,
}

pub fn process_test_file(
    ctx: &mut ThreadCtx,
    relative_path: &str,
) -> (
    TestKind,
    TestFlags,
    TestStatus,
    SubtestCounts,
    Vec<SubtestResult>,
) {
    debug!("Processing test file: {relative_path}");

    let file_contents = match fs::read_to_string(ctx.wpt_dir.join(relative_path)) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::InvalidData => {
            // Tests encoded as UTF-16 or a legacy encoding are not supported: skip them.
            warn!("Skipping {relative_path}: not valid UTF-8");
            return (
                TestKind::Unknown,
                TestFlags::empty(),
                TestStatus::Skip,
                SubtestCounts::ZERO_OF_ZERO,
                Vec::new(),
            );
        }
        Err(err) => panic!("Failed to read {relative_path}: {err}"),
    };

    // Compute flags
    let mut flags = TestFlags::empty();
    if ctx.float_re.is_match(&file_contents) {
        flags |= TestFlags::USES_FLOAT;
    }
    if ctx.intrinsic_re.is_match(&file_contents) {
        flags |= TestFlags::USES_INTRINSIC_SIZE;
    }
    if ctx.calc_re.is_match(&file_contents) {
        flags |= TestFlags::USES_CALC;
    }
    if ctx.direction_re.is_match(&file_contents) {
        flags |= TestFlags::USES_DIRECTION;
    }
    if ctx.writing_mode_re.is_match(&file_contents) {
        flags |= TestFlags::USES_WRITING_MODE;
    }
    if ctx.subgrid_re.is_match(&file_contents) {
        flags |= TestFlags::USES_SUBGRID;
    }
    if ctx.grid_lanes_re.is_match(&file_contents) {
        flags |= TestFlags::USES_GRID_LANES;
    }
    if ctx.script_re.is_match(&file_contents) {
        flags |= TestFlags::USES_SCRIPT;
    }

    // Crash Test
    if is_crash_test(relative_path) {
        // Blitz doesn't run JavaScript, so skip crashtests which rely on it
        if flags.contains(TestFlags::USES_SCRIPT) {
            return (
                TestKind::Crash,
                flags,
                TestStatus::Skip,
                SubtestCounts::ZERO_OF_ZERO,
                Vec::new(),
            );
        }

        let counts = process_crash_test(ctx, relative_path, &file_contents);
        let status = counts.as_status();
        return (TestKind::Crash, flags, status, counts, Vec::new());
    }

    // Ref Test
    let reference = ctx
        .reftest_re
        .captures(&file_contents)
        .and_then(|captures| captures.get(1).map(|href| href.as_str().to_string()));
    if let Some(reference) = reference {
        let counts = process_ref_test(
            ctx,
            relative_path,
            file_contents.as_str(),
            reference.as_str(),
            &mut flags,
        );

        let status = counts.as_status();
        return (TestKind::Ref, flags, status, counts, Vec::new());
    }

    // Attr Test
    let mut matches = ctx.attrtest_re.captures_iter(&file_contents);
    let first = matches.next();
    let second = matches.next();
    if first.is_some() && second.is_none() {
        // TODO: handle tests with multiple calls to checkLayout.
        #[allow(clippy::unnecessary_unwrap)]
        let captures = first.unwrap();
        let selector = captures.get(1).unwrap().as_str().to_string();
        drop(matches);

        debug!("{selector}");

        let (status, counts, results) =
            process_attr_test(ctx, &selector, &file_contents, relative_path);

        return (TestKind::Attr, flags, status, counts, results);
    }

    // Testharness (testharness.js) tests. Blitz doesn't run JavaScript, so these are
    // classified but not run.
    if ctx.testharness_re.is_match(&file_contents) {
        return (
            TestKind::TestHarness,
            flags,
            TestStatus::Skip,
            SubtestCounts::ZERO_OF_ZERO,
            Vec::new(),
        );
    }

    // TODO: Handle other test formats.
    (
        TestKind::Unknown,
        flags,
        TestStatus::Skip,
        SubtestCounts::ZERO_OF_ZERO,
        Vec::new(),
    )
}

fn is_crash_test(relative_path: &str) -> bool {
    relative_path.split('/').any(|seg| seg == "crashtests")
        || [".html", ".htm", ".xht", ".xhtml"]
            .iter()
            .any(|ext| relative_path.ends_with(&format!("-crash{ext}")))
}

fn parse_and_resolve_document(
    ctx: &mut ThreadCtx,
    html: &str,
    relative_path: &str,
) -> BaseDocument {
    ctx.net_provider.reset();
    let mut document = HtmlDocument::from_html(
        html,
        DocumentConfig {
            base_url: Some(ctx.dummy_base_url.join(relative_path).unwrap().to_string()),
            font_ctx: Some(ctx.font_ctx.clone()),
            net_provider: Some(Arc::clone(&ctx.net_provider) as _),
            navigation_provider: Some(Arc::clone(&ctx.navigation_provider)),
            ..Default::default()
        },
    );

    document.as_mut().set_viewport(ctx.viewport.clone());
    document.as_mut().resolve(0.0);

    // Load resources.
    // Loop because loading a resource may result in further resources being requested
    let start = Instant::now();
    while ctx.net_provider.pending_item_count() > 0 {
        ctx.net_provider.for_each(|_| {});
        document.as_mut().resolve(0.0);
        if Instant::now().duration_since(start).as_millis() > 500 {
            ctx.net_provider.log_pending_items();
            panic!(
                "Timeout. {} pending items.",
                ctx.net_provider.pending_item_count()
            );
        }
    }

    document.as_mut().resolve(0.0);

    document.into()
}
