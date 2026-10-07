use blitz_test_harness::{Harness, Rect};

fn page(direction: &str, content: &str) -> Harness {
    Harness::from_html(&format!(
        "<style>body {{ margin: 0 }}</style>\
         <div id=root style='display: flow-root; width: 300px; direction: {direction}'>\
         {content}</div>"
    ))
}

#[test]
fn taller_opposite_side_float_narrows_bfc_below_earlier_float() {
    for direction in ["ltr", "rtl"] {
        for (side, opposite, slot_x) in [("left", "right", 0.0), ("right", "left", 50.0)] {
            let harness = page(
                direction,
                &format!(
                    "<div style='float: {side}; width: 100px; height: 20px'></div>\
                     <div style='float: {opposite}; width: 50px; height: 40px'></div>\
                     <div style='height: 25px'></div>\
                     <div id=slot style='display: flow-root; height: 10px'></div>\
                     <div id=cleared style='clear: both; height: 10px'></div>"
                ),
            );

            assert_eq!(
                harness.layout_rect("#slot"),
                Rect {
                    x: slot_x,
                    y: 25.0,
                    width: 250.0,
                    height: 10.0
                },
                "{direction}, {side}"
            );
            assert_eq!(harness.layout_rect("#cleared").y, 40.0);
        }
    }
}

#[test]
fn taller_same_side_float_keeps_later_float_and_bfc_beside_its_tail() {
    for direction in ["ltr", "rtl"] {
        for (side, float_x, slot_x) in [("left", 130.0, 150.0), ("right", 150.0, 0.0)] {
            let harness = page(
                direction,
                &format!(
                    "<div style='float: {side}; width: 100px; height: 20px'></div>\
                     <div style='float: {side}; width: 30px; height: 40px'></div>\
                     <div style='height: 25px'></div>\
                     <div id=later style='float: {side}; width: 20px; height: 10px'></div>\
                     <div id=slot style='display: flow-root; height: 10px'></div>"
                ),
            );

            assert_eq!(
                harness.layout_rect("#later"),
                Rect {
                    x: float_x,
                    y: 25.0,
                    width: 20.0,
                    height: 10.0
                },
                "{direction}, {side}"
            );
            assert_eq!(
                harness.layout_rect("#slot"),
                Rect {
                    x: slot_x,
                    y: 25.0,
                    width: 150.0,
                    height: 10.0
                },
                "{direction}, {side}"
            );
        }
    }
}

#[test]
fn float_tail_pushes_wide_later_float_down() {
    for direction in ["ltr", "rtl"] {
        for (side, opposite, float_x) in [("left", "right", 0.0), ("right", "left", 40.0)] {
            let harness = page(
                direction,
                &format!(
                    "<div style='float: {side}; width: 100px; height: 20px'></div>\
                     <div style='float: {opposite}; width: 50px; height: 40px'></div>\
                     <div style='height: 25px'></div>\
                     <div id=later style='float: {side}; width: 260px; height: 10px'></div>"
                ),
            );

            assert_eq!(
                harness.layout_rect("#later"),
                Rect {
                    x: float_x,
                    y: 40.0,
                    width: 260.0,
                    height: 10.0
                },
                "{direction}, {side}"
            );
            assert_eq!(harness.layout_rect("#root").height, 50.0);
        }
    }
}

#[test]
fn float_tail_wraps_inline_content_below_earlier_float() {
    for direction in ["ltr", "rtl"] {
        for (side, opposite) in [("left", "right"), ("right", "left")] {
            let harness = page(
                direction,
                &format!(
                    "<div style='float: {side}; width: 100px; height: 20px'></div>\
                     <div style='float: {opposite}; width: 50px; height: 40px'></div>\
                     <div style='height: 25px'></div>\
                     <div style='font-size: 0; line-height: 10px'>\
                     <span style='display: inline-block; vertical-align: top; width: 100px; height: 10px'></span> \
                     <span style='display: inline-block; vertical-align: top; width: 100px; height: 10px'></span> \
                     <span id=last style='display: inline-block; vertical-align: top; width: 100px; height: 10px'></span>\
                     </div>"
                ),
            );

            assert_eq!(harness.layout_rect("#last").y, 35.0, "{direction}, {side}");
        }
    }
}
