//! Repro: blitz lays out `input[type=text]` as a leaf with a line-height
//! content box, but `input[type=number]` falls through that match arm
//! (`layout/mod.rs` lists text/password/email/tel/url/search and *not*
//! number). The editor is still created (construct.rs includes number), so
//! typing works and oninput fires — but the content box can collapse to
//! zero and `draw_text_input_text` clips glyphs to that box, so the field
//! looks empty.

use blitz_dom::Document;
use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use blitz_traits::shell::{ColorScheme, Viewport};

fn build(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn input_boxes(type_attr: &str) -> (f32, f32, f32, f32) {
    let html = format!(
        r#"<!DOCTYPE html><html><head><style>
            body {{ margin: 0; }}
            input {{
                display: block;
                width: 300px;
                padding: 8px 12px;
                border: 1px solid #444;
                font-size: 14px;
                line-height: 1.5;
                background: #334155;
                color: #fff;
            }}
        </style></head><body>
            <input id="f" type="{type_attr}" value="22" />
        </body></html>"#
    );
    let doc = build(&html);
    let node_id = doc.query_selector("#f").expect("selector").expect("#f");
    let node = doc.get_node(node_id).expect("node");
    let layout = node.final_layout();
    (
        layout.size.width,
        layout.size.height,
        layout.content_box_width(),
        layout.content_box_height(),
    )
}

#[test]
fn text_input_has_nonzero_content_box() {
    let (w, h, cw, ch) = input_boxes("text");
    println!("type=text: size={w}x{h} content={cw}x{ch}");
    assert!(w > 100.0, "width should be ~300, got {w}");
    assert!(h > 10.0, "height should include line-height, got {h}");
    assert!(
        cw > 0.0 && ch > 0.0,
        "content box must be non-zero so glyphs are not clipped; got {cw}x{ch}"
    );
}

#[test]
fn number_input_should_match_text_input_content_box() {
    let text = input_boxes("text");
    let number = input_boxes("number");
    println!(
        "type=text:   size={}x{} content={}x{}",
        text.0, text.1, text.2, text.3
    );
    println!(
        "type=number: size={}x{} content={}x{}",
        number.0, number.1, number.2, number.3
    );

    assert!(
        number.2 > 0.0 && number.3 > 0.0,
        "type=number content box collapsed to {}x{} — blitz layout/mod.rs \
         omits \"number\" from the text-input leaf-layout arm, so the field \
         paints into a zero content box and typed digits are clipped",
        number.2,
        number.3
    );

    // Heights should be in the same ballpark once number is laid out as text.
    let height_ratio = number.1 / text.1;
    assert!(
        (0.8..=1.2).contains(&height_ratio),
        "number height {} vs text height {} (ratio {height_ratio})",
        number.1,
        text.1
    );
}
