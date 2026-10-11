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
fn default_button_centers_text() {
    let harness = Harness::from_html(
        r#"<!DOCTYPE html><button id="button"
            style="width: 100px; height: 100px; padding: 0; border: 0; font: 16px/20px sans-serif;">Text</button>"#,
    );
    let doc = harness.base();
    let layout = doc
        .get_node(harness.node("#button"))
        .unwrap()
        .element_data()
        .unwrap()
        .inline_layout_data
        .as_ref()
        .unwrap();
    assert!(layout.layout.width() > 0.0);
    assert_eq!(layout.layout.height(), 20.0);
    assert_eq!(layout.block_offset, 40.0);
}

#[test]
fn scrollable_buttons_export_their_content_baseline() {
    let harness = Harness::from_html(
        r#"<!DOCTYPE html><style>
            button { width: 100px; height: 100px; border: 0; padding: 0; font: 16px/20px sans-serif; }
        </style><button id="visible">Text</button><button id="hidden" style="overflow: hidden">Text</button><button id="auto" style="overflow: auto">Text</button>"#,
    );
    let visible = harness.layout_rect("#visible");
    for selector in ["#hidden", "#auto"] {
        assert_eq!(harness.layout_rect(selector).y, visible.y);
    }
}

#[test]
fn layout_containment_suppresses_button_and_block_text_baselines() {
    for tag in ["button", "div"] {
        for contain in ["layout", "content", "strict", "paint", "none"] {
            let harness = Harness::from_html(&format!(
                r#"<!DOCTYPE html><style>
                    #parent {{ display: flex; align-items: baseline; }}
                    #anchor {{ width: 20px; height: 50px; }}
                    #control {{ contain: {contain}; width: 100px; height: 40px;
                        border: 0; padding: 0; font: 16px/20px sans-serif; }}
                </style><div id="parent"><div id="anchor"></div><{tag} id="control">Text</{tag}></div>"#,
            ));
            let anchor = harness.layout_rect("#anchor");
            let control = harness.layout_rect("#control");
            let bottoms_align = control.y + control.height == anchor.y + anchor.height;
            assert_eq!(
                bottoms_align,
                matches!(contain, "layout" | "content" | "strict"),
                "{tag}, contain: {contain}",
            );
        }
    }
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
fn reset_button_still_centers_content() {
    for css in ["all: unset;", "all: unset; display: inline-block;"] {
        let harness = button(css, "display: block;");
        assert_offset(&harness, 0.0, 40.0);
    }
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
fn button_like_inputs_use_block_centering_without_ua_flex_alignment() {
    for input_type in ["button", "submit", "reset"] {
        let harness = Harness::from_html(&format!(
            r#"<!DOCTYPE html><input id="input" type="{input_type}" value="Text"
                style="width: 100px; height: 100px; padding: 0; border: 0; font: 16px/20px sans-serif;">"#
        ));
        let doc = harness.base();
        let node = doc.get_node(harness.node("#input")).unwrap();
        let style = node.layout_style();
        assert_eq!(
            taffy::FlexboxContainerStyle::align_items(&style),
            taffy::AlignItems::STRETCH,
        );
        assert_eq!(
            taffy::FlexboxContainerStyle::justify_content(&style),
            taffy::JustifyContent::NORMAL,
        );
        assert_eq!(node.taffy_display(), taffy::Display::FlowRoot);
        let layout = node
            .element_data()
            .unwrap()
            .inline_layout_data
            .as_ref()
            .unwrap();
        assert_eq!(layout.layout.height(), 20.0);
        assert_eq!(layout.block_offset, 40.0);
    }
}

#[test]
fn author_flex_and_grid_inputs_still_block_layout_their_labels() {
    for input_type in ["button", "submit", "reset"] {
        for display in [
            "inline",
            "block",
            "flex",
            "inline-flex",
            "grid",
            "inline-grid",
            "table-cell",
        ] {
            let harness = Harness::from_html(&format!(
                r#"<!DOCTYPE html><input id="input" type="{input_type}" value="Text"
                    style="display: {display}; width: 100px; height: 100px; padding: 0; border: 0;
                        font: 16px/20px sans-serif; flex-direction: column; justify-content: end;
                        align-items: start; grid-template: 20px / 20px;">"#
            ));
            let node = harness.base().get_node(harness.node("#input")).unwrap();
            assert_eq!(
                node.taffy_display(),
                taffy::Display::FlowRoot,
                "{input_type}, {display}"
            );
            let layout = node
                .element_data()
                .unwrap()
                .inline_layout_data
                .as_ref()
                .unwrap();
            assert_eq!(layout.layout.height(), 20.0);
            assert_eq!(layout.block_offset, 40.0, "{input_type}, {display}");
            let line = layout.layout.lines().next().unwrap();
            assert!(line.metrics().offset > 0.0, "{input_type}, {display}");
        }
    }
}

#[test]
fn button_input_block_alignment_overrides_and_overflow() {
    for input_type in ["button", "submit", "reset"] {
        for (alignment, height, y) in [
            ("normal", 100, 40.0),
            ("start", 100, 0.0),
            ("end", 100, 80.0),
            ("normal", 10, 0.0),
        ] {
            let harness = Harness::from_html(&format!(
                r#"<!DOCTYPE html><input id="input" type="{input_type}" value="Text"
                    style="display: flex; align-content: {alignment}; width: 100px; height: {height}px;
                        padding: 0; border: 0; font: 16px/20px sans-serif;">"#
            ));
            let node = harness.base().get_node(harness.node("#input")).unwrap();
            let layout = node
                .element_data()
                .unwrap()
                .inline_layout_data
                .as_ref()
                .unwrap();
            assert_eq!(
                layout.block_offset, y,
                "{input_type}, {alignment}, {height}"
            );
        }
    }
}

#[test]
fn button_input_display_restyle_preserves_label_centering() {
    let mut harness = Harness::from_html(
        r#"<!DOCTYPE html><style>
            #input { width: 100px; height: 100px; border: 0; padding: 0; font: 16px/20px sans-serif; }
            #input:hover { display: flex; justify-content: end; }
        </style><input id="input" type="button" value="Text">"#,
    );
    harness.base_mut().set_incremental_layout(true);
    let rect = harness.layout_rect("#input");
    assert!(harness.base_mut().set_hover_to(rect.x + 1.0, rect.y + 1.0));
    harness.pump();
    let node = harness.base().get_node(harness.node("#input")).unwrap();
    assert_eq!(node.taffy_display(), taffy::Display::FlowRoot);
    assert_eq!(
        node.element_data()
            .unwrap()
            .inline_layout_data
            .as_ref()
            .unwrap()
            .block_offset,
        40.0
    );
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
    harness.base_mut().set_incremental_layout(true);
    assert_offset(&harness, 0.0, 40.0);
    let rect = harness.layout_rect("#button");
    assert!(harness.base_mut().set_hover_to(rect.x + 1.0, rect.y + 1.0));
    harness.pump();
    assert_offset(&harness, 0.0, 0.0);
}
