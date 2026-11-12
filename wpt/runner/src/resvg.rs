//! resvg's test suite (`crates/resvg/tests`) wrapped as reftests: one SVG per
//! feature with a reference PNG rendered by resvg.
//!
//! `wpt resvg-generate <resvg checkout> <out dir>` writes, under `<out dir>/resvg`:
//!
//! - `tests/`, `resources/`, `fonts/`: copied from resvg
//! - `fonts.conf`: loads only resvg's fonts, with resvg's generic families
//! - `img/<path>.html`: `<img src=test.svg>` vs resvg's PNG (the usvg backend)
//! - `inline/<path>.html`: the SVG inlined vs resvg's PNG (whichever inline
//!   backend the runner uses; `BLITZ_NATIVE_SVG=1` for native)
//! - `parity/<path>.html`: the SVG inlined vs `<img src=test.svg>` (native vs
//!   usvg on the same rasterizer, with `BLITZ_NATIVE_SVG=1`)
//!
//! Run them with `WPT_DIR=<out dir>` and `FONTCONFIG_FILE=<out dir>/resvg/fonts.conf`,
//! then `wpt resvg-summary` prints per-directory pass counts (`just wpt-resvg`
//! does all of this).

use html_escape::{encode_double_quoted_attribute, encode_text};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// See `ThresholdTolerance`. resvg's references come from tiny-skia, not
/// vello_cpu, so anti-aliased edges and gradients differ slightly almost
/// everywhere: allow 0.5% of a 300x300 render to differ by more than 64/255.
const PNG_FUZZY: &str = "64;450";
/// Native vs usvg runs through the same rasterizer.
const PARITY_FUZZY: &str = "16;90";

/// Matches `GLOBAL_FONTDB` in resvg's `crates/resvg/tests/integration/main.rs`.
const FONT_FAMILIES: &[(&str, &str)] = &[
    ("serif", "Noto Serif"),
    ("sans-serif", "Noto Sans"),
    ("cursive", "Yellowtail"),
    ("fantasy", "Sedgwick Ave Display"),
    ("monospace", "Noto Mono"),
];

static PROLOG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)^\s*(<\?xml[^>]*\?>\s*)?(<!--.*?-->\s*)*(<!DOCTYPE[^>\[]*(\[.*?\])?\s*>)?")
        .unwrap()
});
static ENCODING_RE: LazyLock<regex::bytes::Regex> =
    LazyLock::new(|| regex::bytes::Regex::new(r#"^\s*<\?xml[^>]*encoding="([^"]+)""#).unwrap());

pub fn generate(resvg_dir: &Path, out_dir: &Path) {
    let src = resvg_dir.join("crates/resvg/tests");
    let dst = out_dir.join("resvg");
    if dst.exists() {
        fs::remove_dir_all(&dst).unwrap();
    }
    for name in ["tests", "resources", "fonts"] {
        copy_dir(&src.join(name), &dst.join(name));
    }
    write_fonts_conf(&dst);

    let tests_dir = dst.join("tests");
    let mut svgs = Vec::new();
    collect_svgs(&tests_dir, &mut svgs);
    svgs.sort();

    let mut count = 0;
    for svg in svgs {
        let png = svg.with_extension("png");
        if !png.exists() {
            continue;
        }
        let rel = svg.strip_prefix(&tests_dir).unwrap().with_extension("");
        let rel = rel.to_str().unwrap();
        let (width, height) = image::image_dimensions(&png).unwrap();
        let svg_source = decode_xml(&fs::read(&svg).unwrap());
        let svg_source = PROLOG_RE.replace(&svg_source, "");

        // Pages sit at the same depth as the SVG, so relative hrefs inside an
        // inlined SVG (../../../resources/...) resolve the same way.
        let up = "../".repeat(rel.matches('/').count() + 1);
        let img_svg = format!(r#"<img src="{up}tests/{rel}.svg">"#);
        let img_png = format!(r#"<img src="{up}tests/{rel}.png">"#);
        let modes = [
            ("img", img_svg.as_str(), img_png.as_str(), PNG_FUZZY),
            ("inline", &svg_source, &img_png, PNG_FUZZY),
            ("parity", &svg_source, &img_svg, PARITY_FUZZY),
        ];
        for (mode, test_body, ref_body, fuzzy) in modes {
            let test_path = dst.join(mode).join(format!("{rel}.html"));
            let ref_path = dst.join(mode).join(format!("{rel}-ref.html"));
            let ref_name = ref_path.file_name().unwrap().to_str().unwrap();
            fs::create_dir_all(test_path.parent().unwrap()).unwrap();
            let fuzzy_meta = format!(r#"<meta name="blitz-fuzzy" content="{fuzzy}">"#);
            let link = format!(
                r#"<link rel="match" href="{}">"#,
                encode_double_quoted_attribute(ref_name)
            );
            let head = format!("{link}\n{fuzzy_meta}\n");
            fs::write(&test_path, page(rel, &head, test_body, width, height)).unwrap();
            fs::write(&ref_path, page(rel, "", ref_body, width, height)).unwrap();
        }
        count += 1;
    }
    println!("Generated {count} tests in {}", dst.display());
}

/// Prints per-mode, per-directory pass counts for the generated tests from a
/// `wpt_expectations.txt`. "parity where img passes" counts native/usvg parity
/// only for tests whose usvg rendering matches resvg's PNG, so both backends
/// failing alike is not a pass.
pub fn summarize(expectations: &Path) {
    let expectations = fs::read_to_string(expectations).unwrap();
    let mut results = HashMap::new();
    for line in expectations.lines() {
        let mut fields = line.split_whitespace();
        let (Some(name), Some(status)) = (fields.next(), fields.next()) else {
            continue;
        };
        let mut parts = name.splitn(3, '/');
        if let (Some("resvg"), Some(mode), Some(test)) = (parts.next(), parts.next(), parts.next())
        {
            results.insert((mode, test), status == "PASS");
        }
    }

    let mut counts: BTreeMap<(&str, &str), (u32, u32)> = BTreeMap::new();
    for (&(mode, test), &passed) in &results {
        let mut modes = vec![mode];
        if mode == "parity" && results.get(&("img", test)) == Some(&true) {
            modes.push("parity where img passes");
        }
        let dir = test.split('/').next().unwrap();
        for mode in modes {
            for dir in [dir, "Total"] {
                let count = counts.entry((mode, dir)).or_default();
                count.0 += passed as u32;
                count.1 += 1;
            }
        }
    }

    let modes: BTreeSet<_> = counts.keys().map(|(mode, _)| *mode).collect();
    let mut dirs: Vec<_> = counts
        .keys()
        .map(|(_, dir)| *dir)
        .filter(|dir| *dir != "Total")
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    dirs.push("Total");

    let header: Vec<_> = modes.iter().copied().collect();
    println!("| Directory | {} |", header.join(" | "));
    println!("|---|{}", "---:|".repeat(modes.len()));
    for dir in dirs {
        let cells: Vec<_> = modes
            .iter()
            .map(|mode| match counts.get(&(mode, dir)) {
                Some((pass, total)) => format!("{pass} / {total}"),
                None => String::new(),
            })
            .collect();
        println!("| {dir} | {} |", cells.join(" | "));
    }
}

fn page(title: &str, head: &str, body: &str, width: u32, height: u32) -> String {
    format!(
        "<!DOCTYPE html>\n<meta charset=utf-8>\n<title>{}</title>\n{head}\
         <style>body {{ margin: 0 }} body > img, body > svg {{ display: block; width: {width}px; height: {height}px }}</style>\n\
         <body>\n{body}\n",
        encode_text(title)
    )
}

/// Decodes an SVG with the encoding its XML declaration names, as usvg does.
fn decode_xml(data: &[u8]) -> String {
    let encoding = ENCODING_RE
        .captures(data)
        .and_then(|caps| encoding_rs::Encoding::for_label(&caps[1]))
        .unwrap_or(encoding_rs::UTF_8);
    encoding.decode_without_bom_handling(data).0.into_owned()
}

fn write_fonts_conf(dst: &Path) {
    let fonts_dir = std::path::absolute(dst.join("fonts")).unwrap();
    let fonts_dir = encode_text(fonts_dir.to_str().unwrap());
    let aliases: String = FONT_FAMILIES
        .iter()
        .map(|(generic, family)| {
            format!("  <alias><family>{generic}</family><prefer><family>{family}</family></prefer></alias>\n")
        })
        .collect();
    fs::write(
        dst.join("fonts.conf"),
        format!(
            "<?xml version=\"1.0\"?>\n<!DOCTYPE fontconfig SYSTEM \"fonts.dtd\">\n<fontconfig>\n  \
             <dir>{fonts_dir}</dir>\n  <cachedir>{fonts_dir}-cache</cachedir>\n{aliases}</fontconfig>\n"
        ),
    )
    .unwrap();
}

/// Copies a directory tree, replacing `#` in file names with `_` because the
/// runner treats `#` in a test path as a URL fragment.
fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_str().unwrap().replace('#', "_");
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dst.join(name));
        } else {
            fs::copy(entry.path(), dst.join(name)).unwrap();
        }
    }
}

fn collect_svgs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_svgs(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "svg") {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_declared_encoding() {
        let data = b"<?xml version=\"1.0\" encoding=\"windows-1251\"?><svg>\xcf</svg>";
        assert_eq!(
            decode_xml(data),
            "<?xml version=\"1.0\" encoding=\"windows-1251\"?><svg>П</svg>"
        );
        assert_eq!(decode_xml("<svg>é</svg>".as_bytes()), "<svg>é</svg>");
    }

    #[test]
    fn strips_prolog() {
        let svg = "<?xml version=\"1.0\"?>\n<!-- c -->\n<!DOCTYPE svg [<!ENTITY a \"b\">]>\n<svg/>";
        assert_eq!(PROLOG_RE.replace(svg, ""), "\n<svg/>");
    }
}
