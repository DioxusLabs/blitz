//! An <img> with explicit/percentage width AND height must use them — its
//! intrinsic aspect ratio only matters when a dimension is auto.

use blitz_dom::DocumentConfig;
use blitz_dom::node::{ImageData, RasterImageData, SpecialElementData};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn doc_with_wide_image(img_style: &str) -> (HtmlDocument, usize) {
    let html = format!(
        r#"<html><body style="margin:0">
            <div style="width:100px; height:100px; overflow:hidden; position:relative;">
                <img id="img" style="{img_style}" src="https://example.com/x.png">
            </div>
        </body></html>"#
    );
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let img = doc.query_selector("#img").unwrap().expect("#img");
    // Inject a 200x100 (2:1 wide) image as if it had loaded.
    {
        let node = doc.get_node_mut(img).unwrap();
        node.element_data_mut().unwrap().special_data = SpecialElementData::Image(Box::new(
            ImageData::Raster(RasterImageData::new(200, 100, Arc::new(vec![0u8; 200 * 100 * 4]))),
        ));
        node.cache.clear();
    }
    doc.resolve(0.0);
    (doc, img)
}

#[test]
fn wide_image_with_full_width_and_height_fills_the_box() {
    let (doc, img) = doc_with_wide_image("width:100%; height:100%; object-fit:cover;");
    let layout = doc.get_node(img).unwrap().final_layout;
    assert_eq!(
        (layout.size.width, layout.size.height),
        (100.0, 100.0),
        "img with width:100% and height:100% must fill its container"
    );
}

#[test]
fn wide_image_with_auto_height_uses_aspect_ratio() {
    let (doc, img) = doc_with_wide_image("width:100%;");
    let layout = doc.get_node(img).unwrap().final_layout;
    assert_eq!(
        (layout.size.width, layout.size.height),
        (100.0, 50.0),
        "img with auto height must derive it from the intrinsic aspect ratio"
    );
}

#[test]
fn wide_image_fills_aspect_ratio_sized_parent() {
    // The cover-card shape: parent height comes from aspect-ratio, and the
    // image resolves its percentage height against it.
    let html = r#"<html><body style="margin:0">
        <div style="width:100px; aspect-ratio:1/1; overflow:hidden; position:relative;">
            <img id="img" style="width:100%; height:100%; object-fit:cover;" src="https://example.com/x.png">
        </div>
    </body></html>"#;
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let img = doc.query_selector("#img").unwrap().expect("#img");
    {
        let node = doc.get_node_mut(img).unwrap();
        node.element_data_mut().unwrap().special_data = SpecialElementData::Image(Box::new(
            ImageData::Raster(RasterImageData::new(200, 100, Arc::new(vec![0u8; 200 * 100 * 4]))),
        ));
        node.cache.clear();
    }
    doc.resolve(0.0);
    let layout = doc.get_node(img).unwrap().final_layout;
    assert_eq!(
        (layout.size.width, layout.size.height),
        (100.0, 100.0),
        "img must fill the aspect-ratio-sized parent"
    );
}

#[test]
fn wide_image_in_flex_shelf_card() {
    // The Continue Listening card shape: flex row shelf > fixed-width flex
    // item > aspect-ratio square (implicit width) > img 100%/100% cover.
    let html = r#"<html><body style="margin:0">
        <div style="display:flex; overflow-x:auto; gap:20px; width:600px;">
            <div style="flex:none; width:176px;">
                <div style="aspect-ratio:1/1; overflow:hidden; position:relative; border-radius:12px;">
                    <img id="img" style="width:100%; height:100%; object-fit:cover;" src="https://example.com/x.png">
                </div>
            </div>
        </div>
    </body></html>"#;
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    let img = doc.query_selector("#img").unwrap().expect("#img");
    {
        let node = doc.get_node_mut(img).unwrap();
        node.element_data_mut().unwrap().special_data = SpecialElementData::Image(Box::new(
            ImageData::Raster(RasterImageData::new(320, 180, Arc::new(vec![0u8; 320 * 180 * 4]))),
        ));
        node.cache.clear();
    }
    doc.resolve(0.0);
    let layout = doc.get_node(img).unwrap().final_layout;
    assert_eq!(
        (layout.size.width, layout.size.height),
        (176.0, 176.0),
        "cover img must fill the square card"
    );
}

