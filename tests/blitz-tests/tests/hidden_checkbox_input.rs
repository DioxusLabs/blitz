//! Checkboxes that never generate a box (`display: none` inside an inline
//! `<label>`, as in DuckDuckGo's image CAPTCHA) can still be toggled by clicking
//! their label.

use blitz_test_harness::Harness;

const HTML: &str = r#"<html><head><style>
    input { display: none }
    input:checked { display: block; position: absolute }
    img { display: inline-block; width: 100px; height: 100px }
</style></head><body><div>
    <label for="a"><input type="checkbox" id="a"><img id="img-a" src="data:,"></label>
</div></body></html>"#;

#[test]
fn clicking_label_toggles_hidden_checkbox() {
    let mut h = Harness::from_html(HTML);
    assert!(h.query("#a:checked").is_none());

    h.click("#img-a");
    assert!(h.query("#a:checked").is_some());

    h.click("#img-a");
    assert!(h.query("#a:checked").is_none());
}
