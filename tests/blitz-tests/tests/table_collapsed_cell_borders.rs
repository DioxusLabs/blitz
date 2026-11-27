//! A cell's own border in a `border-collapse: collapse` table whose collapsed
//! grid has no width.
//!
//! Blitz approximates collapsed borders as one uniform grid derived from the
//! first cell, and zeroes every cell's own border (see #504). When that first
//! cell has no border the grid is zero-width, so a border authored on any
//! other single cell vanished entirely.
//!
//! HTML email often collapses layout tables and then draws rules on single
//! cells: a signature's `border-top` divider, a footer line, a row separator.
//!
//! This keeps cells' own borders when the collapsed grid is empty. They are
//! laid out and painted as in the separated model with zero spacing. That
//! restores rules that were otherwise missing; it is not the collapsed model's
//! geometry (an edge border sits wholly inside its cell, and adjacent borders
//! sit side by side instead of merging). Full conflict resolution is #504.

use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_paint::paint_scene;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

const W: u32 = 200;
const H: u32 = 120;

fn doc(html: &str) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(W, H, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn pixel(doc: &mut HtmlDocument, x: u32, y: u32) -> [u8; 3] {
    let buffer = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, doc, 1.0, W, H, 0, 0),
        W,
        H,
    );
    let idx = ((y * W + x) * 4) as usize;
    [buffer[idx], buffer[idx + 1], buffer[idx + 2]]
}

const DIVIDER: &str = r#"<html><body style="margin:0; background:#fff">
    <table style="border-collapse:collapse; width:200px">
        <tr><td style="height:40px; padding:0">name</td></tr>
        <tr><td id="rule" style="border-top:4px solid #ff0000; height:40px; padding:0">contact</td></tr>
    </table>
</body></html>"#;

#[test]
fn single_cell_border_is_laid_out() {
    let doc = doc(DIVIDER);
    let id = doc.query_selector("#rule").unwrap().unwrap();
    let layout = doc.get_node(id).unwrap().final_layout();
    assert_eq!(layout.border.top, 4.0);
}

#[test]
fn single_cell_border_is_painted() {
    let mut doc = doc(DIVIDER);
    let id = doc.query_selector("#rule").unwrap().unwrap();
    let node = doc.get_node(id).unwrap();
    let y = node.absolute_position(0.0, 0.0).y as u32 + 1;
    assert_eq!(pixel(&mut doc, 100, y), [255, 0, 0]);
}

#[test]
fn a_bordered_first_cell_still_sets_the_grid() {
    // Unchanged: when the first cell has a border, the table keeps the
    // uniform grid and cells do not carry their own borders as well.
    let doc = doc(r#"<html><body style="margin:0">
            <table style="border-collapse:collapse">
                <tr><td id="a" style="border:2px solid #000; padding:0">a</td>
                    <td id="b" style="border:2px solid #000; padding:0">b</td></tr>
            </table>
        </body></html>"#);
    for sel in ["#a", "#b"] {
        let id = doc.query_selector(sel).unwrap().unwrap();
        assert_eq!(
            doc.get_node(id).unwrap().final_layout().border.top,
            0.0,
            "{sel}"
        );
    }
}

fn width(doc: &HtmlDocument, sel: &str) -> f32 {
    let id = doc.query_selector(sel).unwrap().unwrap();
    doc.get_node(id).unwrap().final_layout().size.width
}

#[test]
fn a_bordered_first_cell_does_not_widen_pixel_columns() {
    // The cells' borders are replaced by the grid, so they must not also be
    // counted in the column widths derived from the cells' `width`.
    let doc = doc(r#"<html><body style="margin:0">
            <table style="border-collapse:collapse">
                <tr><td id="a" style="width:60px; padding:0; border:4px solid #000"></td>
                    <td id="b" style="width:60px; padding:0; border:4px solid #000"></td></tr>
            </table>
        </body></html>"#);
    assert_eq!(width(&doc, "#a"), 60.0);
    assert_eq!(width(&doc, "#b"), 60.0);
}

#[test]
fn a_bordered_first_cell_does_not_widen_fixed_percent_columns() {
    let doc = doc(r#"<html><body style="margin:0">
            <table style="border-collapse:collapse; table-layout:fixed; width:200px">
                <tr><td id="a" style="width:25%; padding:0; border:4px solid #000"></td>
                    <td id="b" style="padding:0; border:4px solid #000"></td></tr>
            </table>
        </body></html>"#);
    // The same widths as before cells could keep their own borders.
    assert_eq!(width(&doc, "#a"), 48.0);
    assert_eq!(width(&doc, "#b"), 140.0);
}
