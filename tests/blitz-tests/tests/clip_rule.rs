use std::sync::Arc;

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{DocumentConfig, LocalName, QualName, ns};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};

fn load(content: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        &format!(r#"<html><body style="margin:0; background:white">{content}</body></html>"#),
        DocumentConfig {
            viewport: Some(Viewport::new(100, 100, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider)),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn assert_clipping(doc: &mut HtmlDocument, has_hole: bool) {
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, doc, 1.0, 100, 100, 0, 0),
        100,
        100,
    );
    let pixel = |x: usize, y: usize| &buffer[(y * 100 + x) * 4..(y * 100 + x) * 4 + 4];
    assert_eq!(pixel(10, 10), &[255, 0, 0, 255]);
    assert_eq!(
        pixel(50, 50),
        if has_hole {
            &[255, 255, 255, 255]
        } else {
            &[255, 0, 0, 255]
        }
    );
}

fn svg(clip_attrs: &str, path_attrs: &str) -> String {
    format!(
        r##"<svg width="100" height="100">
        <defs><clipPath id="clip" {clip_attrs}>
            <path id="clip-shape" {path_attrs} d="M0 0H100V100H0Z M25 25H75V75H25Z"/>
        </clipPath></defs>
        <rect fill="red" width="100" height="100" clip-path="url(#clip)"/>
    </svg>"##
    )
}

#[test]
fn svg_clip_rule_controls_pixels() {
    for (attrs, has_hole) in [
        ("", false),
        (r#"clip-rule="nonzero""#, false),
        (r#"clip-rule="evenodd""#, true),
        (r#"clip-rule="EVENODD /* comment */""#, true),
        (r#"clip-rule="even\6f dd""#, true),
        (r#"clip-rule="invalid""#, false),
        (r#"clip-rule="evenodd nonzero""#, false),
        (r#"fill-rule="evenodd""#, false),
        (r#"clip-rule="nonzero" style="clip-rule:evenodd""#, true),
    ] {
        assert_clipping(&mut load(&svg("", attrs)), has_hole);
    }
}

#[test]
fn svg_clip_rule_inherits_from_clip_path() {
    assert_clipping(&mut load(&svg(r#"style="clip-rule:evenodd""#, "")), true);
    assert_clipping(&mut load(&svg(r#"clip-rule="evenodd""#, "")), true);
    assert_clipping(
        &mut load(&svg(r#"clip-rule="evenodd""#, r#"clip-rule="inherit""#)),
        true,
    );
    assert_clipping(
        &mut load(&svg(r#"clip-rule="evenodd""#, r#"clip-rule="initial""#)),
        false,
    );
    assert_clipping(
        &mut load(&svg(
            r#"clip-rule="evenodd""#,
            r#"style="clip-rule:nonzero""#,
        )),
        false,
    );
}

#[test]
fn svg_clip_rule_updates_after_attribute_change() {
    let mut doc = load(&svg("", r#"clip-rule="nonzero""#));
    assert_clipping(&mut doc, false);
    let path = doc.query_selector("#clip-shape").unwrap().unwrap();
    doc.mutate().set_attribute(
        path,
        QualName {
            prefix: None,
            ns: ns!(),
            local: LocalName::from("clip-rule"),
        },
        "evenodd",
    );
    doc.resolve(0.0);
    assert_clipping(&mut doc, true);
}

#[test]
fn svg_clip_rule_cascades_from_document_stylesheets() {
    let cases = [
        (
            "#clip-shape { clip-rule:evenodd }",
            r#"clip-rule="nonzero""#,
            true,
        ),
        ("svg { clip-rule:evenodd }", "", true),
        ("body { clip-rule:evenodd }", "", true),
        (
            "svg { clip-rule:evenodd } #clip-shape { clip-rule:initial }",
            "",
            false,
        ),
        (
            "#clip-shape { clip-rule:nonzero !important }",
            r#"style="clip-rule:evenodd""#,
            false,
        ),
        (
            "#clip-shape { clip-rule:evenodd }",
            r#"style="clip-rule:nonzero""#,
            false,
        ),
        ("#clip-shape { clip-rule:invalid }", "", false),
        (
            "svg { --rule:evenodd } #clip-shape { clip-rule:var(--rule) }",
            "",
            true,
        ),
    ];
    for (css, attrs, has_hole) in cases {
        let content = format!("<style>{css}</style>{}", svg("", attrs));
        assert_clipping(&mut load(&content), has_hole);
    }
}

#[test]
fn svg_clip_rule_updates_after_class_change() {
    let content = format!(
        "<style>.hole {{ clip-rule:evenodd }}</style>{}",
        svg("", "")
    );
    let mut doc = load(&content);
    assert_clipping(&mut doc, false);
    let path = doc.query_selector("#clip-shape").unwrap().unwrap();
    doc.mutate().set_attribute(
        path,
        QualName {
            prefix: None,
            ns: ns!(),
            local: LocalName::from("class"),
        },
        "hole",
    );
    doc.resolve(0.0);
    assert_clipping(&mut doc, true);
}

#[test]
fn css_clip_path_uses_its_own_fill_rule() {
    for (rule, has_hole) in [("nonzero", false), ("evenodd", true)] {
        for shape in [
            format!("path({rule}, 'M0 0H100V100H0Z M25 25H75V75H25Z')"),
            format!(
                "polygon({rule}, 0 0, 100% 0, 100% 100%, 0 100%, 0 0, 25% 25%, 75% 25%, 75% 75%, 25% 75%, 25% 25%, 0 0)"
            ),
            format!(
                "shape({rule} from 0px 0px, hline to 100px, vline to 100px, hline to 0px, close, move to 25px 25px, hline to 75px, vline to 75px, hline to 25px, close)"
            ),
        ] {
            let content = format!(
                r#"<div style="width:100px; height:100px; background:red; clip-rule:evenodd; clip-path:{shape}"></div>"#
            );
            assert_clipping(&mut load(&content), has_hole);
        }
    }
}
