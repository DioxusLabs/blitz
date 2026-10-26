use blitz_test_harness::Harness;

#[test]
fn number_inputs_use_text_input_intrinsic_size() {
    for value in ["", "42", "-123.5"] {
        let harness = Harness::from_html(&format!(
            r#"<!DOCTYPE html><style>
                input {{ font: 16px/24px sans-serif; padding: 8px; border: 1px solid; }}
            </style>
            <input id="number" type="number" value="{value}">
            <input id="text" type="text" value="{value}">
            <input id="email" type="email" value="{value}">
            <input id="search" type="search" value="{value}">"#,
        ));
        let number = harness.layout_rect("#number");
        assert_eq!(number.height, 42.0);
        for selector in ["#text", "#email", "#search"] {
            let other = harness.layout_rect(selector);
            assert_eq!((number.width, number.height), (other.width, other.height));
        }
    }
}
