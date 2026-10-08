use blitz_dom::{BaseDocument, QualName, ns};
use blitz_test_harness::Harness;
use blitz_traits::shell::{ColorScheme, Viewport};

fn snapshot(doc: &BaseDocument) -> Vec<String> {
    doc.tree()
        .iter()
        .filter(|(_, node)| node.element_data().is_some() || node.is_anonymous())
        .map(|(_, node)| {
            let mut result = format!("{:?}", node.final_layout());
            if let Some(text) = node
                .element_data()
                .and_then(|el| el.inline_layout_data.as_ref())
            {
                result.push_str(&format!(" {:?} {}", text.text, text.block_offset));
                for line in text.layout.lines() {
                    result.push_str(&format!(" {:?} {:?}", line.text_range(), line.metrics()));
                }
            }
            result
        })
        .collect()
}

fn assert_relayout_equivalent(harness: &mut Harness) {
    let mut doc = harness.base_mut();
    let initial = snapshot(&doc);
    let ids: Vec<_> = doc.tree().iter().map(|(id, _)| id).collect();
    for _ in 0..3 {
        for id in &ids {
            doc.get_node_mut(*id).unwrap().clear_layout_cache();
        }
        doc.resolve_layout();
        assert_eq!(initial, snapshot(&doc), "same-width relayout");
    }
    for id in &ids {
        doc.get_node_mut(*id).unwrap().invalidate_layout_cache();
    }
    doc.resolve_layout();
    assert_eq!(initial, snapshot(&doc), "forced line breaking");
}

#[test]
fn repeated_alignment_is_idempotent() {
    for direction in ["ltr", "rtl"] {
        for alignment in ["start", "center", "end", "justify"] {
            let mut harness = Harness::from_html(&format!(
                "<div style='display:flow-root;width:143px;direction:{direction};text-align:{alignment};text-align-last:justify'>one two three four five six seven eight nine ten eleven twelve</div>"
            ));
            assert_relayout_equivalent(&mut harness);
        }
    }
}

#[test]
fn width_indent_and_style_changes_do_not_reuse_stale_lines() {
    let mut harness = Harness::from_html(
        "<div id='test' style='display:flow-root;box-sizing:border-box;width:200px;text-indent:20%'>one two three four five six seven eight nine ten eleven twelve</div>",
    );
    let id = harness.node("#test");
    assert_relayout_equivalent(&mut harness);
    for style in [
        "display:flow-root;box-sizing:border-box;width:240px;padding:0 20px;text-indent:20%",
        "display:flow-root;width:100px;text-indent:-10px;text-align:justify",
        "display:flow-root;width:200px;text-indent:20%;font-size:24px",
        "display:flow-root;width:200px;text-indent:20%;line-break:anywhere",
        "display:flow-root;width:200px;text-indent:20%;white-space:pre-wrap",
    ] {
        harness.base_mut().mutate().set_attribute(
            id,
            QualName::new(None, ns!(), "style".into()),
            style,
        );
        harness.pump();
        assert_relayout_equivalent(&mut harness);
    }
}

#[test]
fn text_changes_do_not_reuse_stale_lines() {
    let mut harness = Harness::from_html(
        "<div id='test' style='display:flow-root;width:110px'>one two three</div>",
    );
    let id = harness.node("#test");
    let text_id = harness.base().get_node(id).unwrap().children[0];
    for text in [
        "one two three four five six seven eight",
        "short",
        "different words with similar length",
    ] {
        harness.base_mut().mutate().set_node_text(text_id, text);
        harness.pump();
        assert_relayout_equivalent(&mut harness);
    }
}

#[test]
fn percentage_indent_changes_with_an_unchanged_content_width() {
    let mut harness = Harness::from_html(
        "<div id='outer' style='width:300px'><div style='display:flow-root;box-sizing:border-box;width:100%;padding-left:calc(100% - 200px);text-indent:10%'>one two three four five six seven eight nine ten eleven twelve</div></div>",
    );
    let outer = harness.node("#outer");
    for width in [300, 400, 250, 300] {
        harness
            .base_mut()
            .mutate()
            .set_style_property(outer, "width", &format!("{width}px"));
        harness.pump();
        assert_relayout_equivalent(&mut harness);
    }
}

#[test]
fn inherited_floats_do_not_reuse_lines_at_the_same_width() {
    let mut harness = Harness::from_html(
        "<div style='width:220px'><div id='float' style='float:left;width:70px;height:100px'></div><div>one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen</div></div>",
    );
    let float = harness.node("#float");
    for width in [70, 120, 20, 70] {
        harness
            .base_mut()
            .mutate()
            .set_style_property(float, "width", &format!("{width}px"));
        harness.pump();
        // Compare the two line-breaking paths starting from a full layout.
        {
            let mut doc = harness.base_mut();
            let ids: Vec<_> = doc.tree().iter().map(|(id, _)| id).collect();
            for id in ids {
                doc.get_node_mut(id).unwrap().clear_layout_cache();
            }
            doc.resolve_layout();
        }
        assert_relayout_equivalent(&mut harness);
    }
}

#[test]
fn inline_boxes_floats_and_slot_changes_match_full_breaking() {
    for content in [
        "one two <span style='display:inline-block;width:50px;height:20px'>box</span> three four five six",
        "<span style='float:left;width:70px;height:60px'>float</span>one two three four five six seven eight nine ten",
        "one two <span style='position:absolute'>absolute</span> three four five six",
        "one two three four five six seven eight nine ten",
    ] {
        let mut harness = Harness::from_html(&format!(
            "<div id='outer' style='width:300px'><div style='width:180px;padding:10px'>{content}</div></div>"
        ));
        let outer = harness.node("#outer");
        for width in [300, 220, 400, 300] {
            harness
                .base_mut()
                .mutate()
                .set_style_property(outer, "width", &format!("{width}px"));
            harness.pump();
            assert_relayout_equivalent(&mut harness);
        }
    }
}

#[test]
fn float_free_first_line_uses_content_width() {
    let mut harness = Harness::from_html(
        "<div id='outer' style='width:300px'><div id='inner' style='box-sizing:border-box;width:100px;padding-left:50%'>one two three four five six</div></div>",
    );
    let outer = harness.node("#outer");
    let inner = harness.node("#inner");
    for width in [300, 400, 200, 300] {
        harness
            .base_mut()
            .mutate()
            .set_style_property(outer, "width", &format!("{width}px"));
        harness.pump();
        {
            let doc = harness.base();
            let layout = doc.get_node(inner).unwrap().element_data().unwrap();
            let first_line = layout
                .inline_layout_data
                .as_ref()
                .unwrap()
                .layout
                .get(0)
                .unwrap();
            assert_eq!(first_line.metrics().inline_max_coord, 0.0);
        }
        assert_relayout_equivalent(&mut harness);
    }
}

#[test]
fn float_free_fractional_width_matches_full_breaking() {
    let mut harness = Harness::from_html(
        "<div id='outer' style='width:300px'><div id='inner' style='width:180.3px;padding:10.7px'>one two three four five six</div></div>",
    );
    let inner = harness.node("#inner");
    {
        let doc = harness.base();
        let layout = doc.get_node(inner).unwrap().element_data().unwrap();
        let first_line = layout
            .inline_layout_data
            .as_ref()
            .unwrap()
            .layout
            .get(0)
            .unwrap();
        assert_eq!(first_line.metrics().inline_max_coord, 180.3);
    }
    assert_relayout_equivalent(&mut harness);
}

#[test]
fn intrinsic_sizing_and_scale_round_trips_match_full_breaking() {
    for display in ["flex", "grid", "table"] {
        let mut harness = Harness::from_html(&format!(
            "<div style='display:{display};width:210px'><div>one two three four five six seven eight nine ten eleven twelve</div></div>"
        ));
        for scale in [1.0, 1.25, 2.0, 1.0] {
            harness
                .base_mut()
                .set_viewport(Viewport::new(800, 600, scale, ColorScheme::Light));
            harness.pump();
            assert_relayout_equivalent(&mut harness);
        }
    }
}
