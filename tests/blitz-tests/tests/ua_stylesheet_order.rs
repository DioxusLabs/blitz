//! The default user-agent stylesheet cascades before caller-provided ones, so a
//! caller's UA rules win conflicts with it.

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
fn default_sheet_is_inserted_first() {
    let custom = UserAgentStylesheet::parse("body { margin: 0 }");
    let config = DocumentConfig {
        ua_stylesheets: vec![custom.clone()],
        ..Default::default()
    };
    let doc = HtmlDocument::from_html(HTML, config).into_inner();
    let sheets: Vec<_> = doc.useragent_stylesheets().cloned().collect();
    assert_eq!(
        sheets,
        [
            UserAgentStylesheet::default_sheet().sheet().clone(),
            custom.sheet().clone()
        ]
    );
}

#[test]
fn custom_sheet_overrides_default_sheet() {
    assert_eq!(body_x(vec![]), 8.0);
    assert_eq!(
        body_x(vec![UserAgentStylesheet::parse("body { margin: 0 }")]),
        0.0
    );
}

#[test]
fn explicit_default_position_is_kept() {
    let custom = UserAgentStylesheet::parse("body { margin: 0 }");
    assert_eq!(
        body_x(vec![custom, UserAgentStylesheet::default_sheet()]),
        8.0
    );
}
