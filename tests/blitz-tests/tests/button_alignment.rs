use blitz_test_harness::Harness;

fn button(css: &str, child_css: &str) -> Harness {
    Harness::from_html(&format!(
        r#"<!DOCTYPE html><style>
            body {{ margin: 0; }}
            #button {{ {css} width: 100px; height: 100px; padding: 0; border: 0;
                font-size: 0; line-height: 0; }}
            #child {{ width: 20px; height: 20px; {child_css} }}
        </style><button id="button"><span id="child"></span></button>"#
    ))
}

fn assert_offset(harness: &Harness, x: f32, y: f32) {
    let button = harness.layout_rect("#button");
    let child = harness.layout_rect("#child");
    assert_eq!((child.x - button.x, child.y - button.y), (x, y));
}

#[test]
fn default_button_centers_inline_content() {
    let harness = button("", "display: inline-block; vertical-align: top;");
    assert_offset(&harness, 40.0, 40.0);
}

#[test]
fn block_button_centers_block_children() {
    for css in ["", "display: block;", "display: flow-root;"] {
        let harness = button(css, "display: block;");
        assert_offset(&harness, 0.0, 40.0);
    }
}

#[test]
fn overflowing_button_uses_safe_center() {
    for child_css in [
        "display: block; height: 120px;",
        "display: inline-block; height: 120px; vertical-align: top;",
    ] {
        let harness = button("", child_css);
        let button = harness.layout_rect("#button");
        let child = harness.layout_rect("#child");
        assert_eq!(child.y - button.y, 0.0);
    }
}

#[test]
fn author_flex_alignment_is_not_centered_by_ua_styles() {
    for display in ["flex", "inline-flex"] {
        let harness = button(
            &format!("display: {display}; flex-direction: column; align-items: flex-start;"),
            "",
        );
        assert_offset(&harness, 0.0, 0.0);
    }
    let harness = button("display: flex;", "");
    assert_offset(&harness, 0.0, 0.0);
    let harness = button(
        "display: flex; flex-direction: column; align-items: flex-start; justify-content: center;",
        "",
    );
    assert_offset(&harness, 0.0, 40.0);
}

#[test]
fn wrapping_flex_and_grid_use_normal_content_alignment() {
    for css in [
        "display: flex; flex-wrap: wrap; align-items: flex-start;",
        "display: grid; grid-template: 20px / 20px;",
    ] {
        let harness = button(css, "");
        assert_offset(&harness, 0.0, 0.0);
    }
}

#[test]
fn block_alignment_overrides_button_default() {
    for (alignment, y) in [("start", 0.0), ("end", 80.0), ("normal", 40.0)] {
        for child_css in [
            "display: block;",
            "display: inline-block; vertical-align: top;",
        ] {
            let harness = button(&format!("align-content: {alignment};"), child_css);
            let button = harness.layout_rect("#button");
            let child = harness.layout_rect("#child");
            assert_eq!(child.y - button.y, y, "{alignment}, {child_css}");
        }
    }
}

#[test]
fn reset_block_button_still_centers_content() {
    let harness = button("all: unset; display: inline-block;", "display: block;");
    assert_offset(&harness, 0.0, 40.0);
}

#[test]
fn ordinary_blocks_keep_normal_alignment() {
    let harness = Harness::from_html(
        r#"<!DOCTYPE html><div id="button" style="width: 100px; height: 100px;">
            <div id="child" style="width: 20px; height: 20px;"></div>
        </div>"#,
    );
    assert_offset(&harness, 0.0, 0.0);
}

#[test]
fn hover_alignment_override_updates_button_layout() {
    let mut harness = Harness::from_html(
        r#"<!DOCTYPE html><style>
            #button { width: 100px; height: 100px; border: 0; padding: 0; }
            #button:hover { align-content: start; }
            #child { display: block; width: 20px; height: 20px; }
        </style><button id="button"><span id="child"></span></button>"#,
    );
    assert_offset(&harness, 0.0, 40.0);
    let rect = harness.layout_rect("#button");
    assert!(harness.base_mut().set_hover_to(rect.x + 1.0, rect.y + 1.0));
    harness.pump();
    assert_offset(&harness, 0.0, 0.0);
}
