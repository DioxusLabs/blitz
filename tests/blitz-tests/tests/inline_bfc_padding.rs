use blitz_test_harness::Harness;

#[test]
fn float_layout_counts_table_cell_padding_once() {
    let harness = Harness::from_html(
        r#"<table style="border-spacing:0"><tr><td id="cell" style="font-size:10px;line-height:20px;padding:40px 0 10px">x</td></tr></table>"#,
    );

    assert_eq!(harness.layout_rect("#cell").height, 70.0);
}
