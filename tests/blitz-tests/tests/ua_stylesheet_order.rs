//! `DocumentConfig::ua_stylesheets` is used as given, in cascade order, so later
//! UA sheets win conflicts with earlier ones (including the default sheet).

use blitz_dom::{DocumentConfig, UserAgentStylesheet};
use blitz_html::HtmlDocument;

const HTML: &str = "<html><body><div style='height: 10px'></div></body></html>";

fn body_x(ua_stylesheets: Vec<UserAgentStylesheet>) -> f32 {
    let config = DocumentConfig {
        ua_stylesheets,
        ..Default::default()
    };
    let mut doc = HtmlDocument::from_html(HTML, config).into_inner();
    doc.resolve(0.0);
    let body = doc.query_selector("body").unwrap().unwrap();
    doc.get_node(body).unwrap().final_layout().location.x
}

#[test]
fn configured_sheets_are_used_as_given() {
    let custom = UserAgentStylesheet::parse("body { margin: 0 }");
    let config = DocumentConfig {
        ua_stylesheets: vec![custom.clone()],
        ..Default::default()
    };
    let doc = HtmlDocument::from_html(HTML, config).into_inner();
    let sheets: Vec<_> = doc.useragent_stylesheets().cloned().collect();
    assert_eq!(sheets, [custom.sheet().clone()]);
}

#[test]
fn default_config_uses_the_default_sheet() {
    assert_eq!(body_x(DocumentConfig::default().ua_stylesheets), 8.0);
}

#[test]
fn later_sheets_override_earlier_ones() {
    let custom = UserAgentStylesheet::parse("body { margin: 0 }");
    let default = UserAgentStylesheet::default_sheet();
    assert_eq!(body_x(vec![default.clone(), custom.clone()]), 0.0);
    assert_eq!(body_x(vec![custom, default]), 8.0);
}
