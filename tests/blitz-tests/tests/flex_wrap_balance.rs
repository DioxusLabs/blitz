use blitz_dom::{QualName, local_name, ns};
use blitz_test_harness::Harness;

fn page(flex_wrap: &str, extra_style: &str) -> Harness {
    Harness::from_html(&format!(
        r#"<html><head><style>
            body {{ margin: 0; }}
            #flex {{ display: flex; width: 100px; flex-wrap: {flex_wrap}; {extra_style} }}
            #flex > div {{ width: 30px; height: 20px; flex-shrink: 0; }}
        </style></head><body>
            <div id="flex">
                <div id="a"></div><div id="b"></div><div id="c"></div><div id="d"></div>
            </div>
        </body></html>"#
    ))
}

fn positions(harness: &Harness) -> [(f32, f32); 4] {
    ["#a", "#b", "#c", "#d"].map(|selector| {
        let rect = harness.layout_rect(selector);
        (rect.x, rect.y)
    })
}

#[test]
fn balance_distributes_items_evenly_across_lines() {
    for value in ["balance", "wrap balance", "balance wrap"] {
        let harness = page(value, "");
        assert_eq!(
            positions(&harness),
            [(0.0, 0.0), (30.0, 0.0), (0.0, 20.0), (30.0, 20.0)],
            "flex-wrap: {value}"
        );
    }
}

#[test]
fn balance_wrap_reverse_reverses_balanced_lines() {
    for value in ["wrap-reverse balance", "balance wrap-reverse"] {
        let harness = page(value, "");
        assert_eq!(
            positions(&harness),
            [(0.0, 20.0), (30.0, 20.0), (0.0, 0.0), (30.0, 0.0)],
            "flex-wrap: {value}"
        );
    }
}

#[test]
fn existing_wrap_modes_keep_their_line_breaks() {
    for (value, expected) in [
        (
            "nowrap",
            [(0.0, 0.0), (30.0, 0.0), (60.0, 0.0), (90.0, 0.0)],
        ),
        ("wrap", [(0.0, 0.0), (30.0, 0.0), (60.0, 0.0), (0.0, 20.0)]),
        (
            "wrap-reverse",
            [(0.0, 20.0), (30.0, 20.0), (60.0, 20.0), (0.0, 0.0)],
        ),
    ] {
        assert_eq!(positions(&page(value, "")), expected, "flex-wrap: {value}");
    }
}

#[test]
fn flex_line_count_requests_a_minimum_number_of_balanced_lines() {
    let harness = page("balance", "flex-line-count: 4;");
    assert_eq!(
        positions(&harness),
        [(0.0, 0.0), (0.0, 20.0), (0.0, 40.0), (0.0, 60.0)]
    );
}

#[test]
fn changing_flex_wrap_and_line_count_relayouts() {
    for incremental in [false, true] {
        let mut harness = page("wrap", "");
        harness.base_mut().set_incremental_layout(incremental);
        let flex = harness.node("#flex");
        let style = QualName::new(None, ns!(), local_name!("style"));

        harness.base_mut().mutate().set_attribute(
            flex,
            style.clone(),
            "flex-wrap: balance; flex-line-count: 4;",
        );
        harness.pump();
        assert_eq!(
            positions(&harness),
            [(0.0, 0.0), (0.0, 20.0), (0.0, 40.0), (0.0, 60.0)]
        );

        harness.base_mut().mutate().set_attribute(
            flex,
            style,
            "flex-wrap: balance wrap-reverse; flex-line-count: 2;",
        );
        harness.pump();
        assert_eq!(
            positions(&harness),
            [(0.0, 20.0), (30.0, 20.0), (0.0, 0.0), (30.0, 0.0)]
        );
    }
}
