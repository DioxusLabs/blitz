use std::sync::LazyLock;
use std::{fs, sync::Arc, time::Instant};

use blitz_dom::{BaseDocument, DocumentConfig};
use blitz_html::HtmlDocument;
use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE, WINDOWS_1252};
use log::debug;
use regex::bytes::Regex as BytesRegex;

use crate::{SubtestCounts, TestFlags, TestKind, TestStatus, ThreadCtx};

mod attr_test;
mod ref_test;

pub use attr_test::process_attr_test;
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

    let file_bytes = fs::read(ctx.wpt_dir.join(relative_path)).unwrap();
    let file_contents = decode_file_bytes(&file_bytes);

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

    // TODO: Handle other test formats.
    (
        TestKind::Unknown,
        flags,
        TestStatus::Skip,
        SubtestCounts::ZERO_OF_ZERO,
        Vec::new(),
    )
}

/// Decode raw bytes from a test file, honouring a BOM or a declared encoding
/// (XML declaration, `<meta charset>`, or `charset` in a `content` attribute).
/// Defaults to UTF-8 with a windows-1252 fallback for undeclared legacy encodings.
fn decode_file_bytes(bytes: &[u8]) -> String {
    if let Some((encoding, bom_len)) = Encoding::for_bom(bytes) {
        return encoding
            .decode_without_bom_handling(&bytes[bom_len..])
            .0
            .into_owned();
    }

    if let Ok(utf8) = std::str::from_utf8(bytes) {
        return utf8.to_owned();
    }

    let encoding = scan_declared_encoding(bytes).unwrap_or(WINDOWS_1252);
    encoding.decode_without_bom_handling(bytes).0.into_owned()
}

/// Scan the first 1024 bytes for an encoding declaration and look up the
/// corresponding encoding. A declared UTF-16 encoding is mapped to UTF-8, as an
/// ASCII-superset declaration being readable means the content is not UTF-16
/// (this matches the HTML spec's "get an encoding" algorithm).
fn scan_declared_encoding(bytes: &[u8]) -> Option<&'static Encoding> {
    static DECLARED_ENCODING_RE: LazyLock<BytesRegex> = LazyLock::new(|| {
        BytesRegex::new(r#"(?i)(?:encoding|charset)\s*=\s*["']?([a-zA-Z0-9_:.-]+)"#).unwrap()
    });

    let prefix = &bytes[..bytes.len().min(1024)];
    let label = DECLARED_ENCODING_RE.captures(prefix)?.get(1)?.as_bytes();
    let encoding = Encoding::for_label(label)?;
    if encoding == UTF_16LE || encoding == UTF_16BE {
        return Some(UTF_8);
    }
    Some(encoding)
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
