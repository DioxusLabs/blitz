use anyrender::render_to_buffer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use blitz_dom::{ScrollBehavior, ScrollLogicalPosition};
use blitz_paint::paint_scene;
use blitz_test_harness::Harness;
use markup5ever::{QualName, local_name, ns};

fn set_style(h: &mut Harness, selector: &str, value: &str) {
    let id = h.node(selector);
    h.base_mut().mutate().set_attribute(
        id,
        QualName::new(None, ns!(), local_name!("style")),
        value,
    );
    h.pump();
}

fn scroll(h: &mut Harness, selector: &str, y: f64) {
    let id = h.node(selector);
    h.base_mut().scroll_to(id, 0.0, y, ScrollBehavior::Instant);
}

fn shift(h: &Harness, selector: &str) -> f32 {
    h.base()
        .get_node(h.node(selector))
        .unwrap()
        .sticky_offset()
        .y
}

const STICKY_HEADER: &str = "<body style='margin:0'><div id=t style='position:sticky;top:0;height:20px'></div><div style='height:2000px'></div></body>";

#[test]
fn removing_sticky_clears_offset() {
    let mut h = Harness::from_html(
        "<body style='margin:0'><div id=s style='overflow:auto;height:100px'><div style='height:100px'></div><div id=t style='position:sticky;top:0;height:20px'></div><div style='height:500px'></div></div>",
    );
    scroll(&mut h, "#s", 150.0);
    assert_eq!(shift(&h, "#t"), 50.0);
    set_style(&mut h, "#t", "position:static;height:20px");
    assert_eq!(shift(&h, "#t"), 0.0);
}

#[test]
fn sticky_row_style_removal_clears_cells() {
    let mut h = Harness::from_html(
        "<body style='margin:0'><div id=s style='height:100px;overflow:auto'><table style='border-spacing:0'><tbody><tr id=r style='position:sticky;top:0'><td id=t style='height:20px;padding:0'>X</td></tr><tr><td style='height:500px;padding:0'></td></tr></tbody></table></div>",
    );
    scroll(&mut h, "#s", 100.0);
    assert_eq!(shift(&h, "#t"), 100.0);
    set_style(&mut h, "#r", "position:static");
    assert_eq!(shift(&h, "#t"), 0.0);
}

#[test]
fn newly_created_sticky_pseudo_is_registered() {
    for pseudo in ["before", "after"] {
        let mut h = Harness::from_html(&format!(
            "<style>#host::{pseudo} {{content:'X';display:block;position:sticky;top:0;height:20px}}</style><body style='margin:0'><div id=s style='height:100px;overflow:auto'><div id=host style='height:500px'></div></div>"
        ));
        let id = {
            let base = h.base();
            let host = base.get_node(h.node("#host")).unwrap();
            if pseudo == "before" {
                host.before()
            } else {
                host.after()
            }
            .unwrap()
        };
        scroll(&mut h, "#s", 100.0);
        assert_eq!(h.base().get_node(id).unwrap().sticky_offset().y, 100.0);
    }
}

#[test]
fn scroll_after_dropping_sticky_before_resolve_does_not_panic() {
    let mut h = Harness::from_html(STICKY_HEADER);
    let t = h.node("#t");
    h.base_mut().mutate().remove_and_drop_node(t);
    h.base_mut()
        .set_viewport_scroll(blitz_dom::util::Point { x: 0.0, y: 100.0 });
    h.pump();
}

#[test]
fn reparented_stickies_resolve_ancestors_first() {
    let mut h = Harness::from_html(
        "<body style='margin:0'><div id=inner style='position:sticky;top:0;height:20px'></div><div style='height:2000px'><div style='height:1000px'><div id=outer style='position:sticky;top:0;height:300px'></div></div></div>",
    );
    let inner = h.node("#inner");
    let outer = h.node("#outer");
    h.base_mut().mutate().append_children(outer, &[inner]);
    h.pump();
    h.base_mut()
        .set_viewport_scroll(blitz_dom::util::Point { x: 0.0, y: 100.0 });
    assert_eq!(shift(&h, "#outer"), 100.0);
    assert_eq!(shift(&h, "#inner"), 0.0);
}

#[test]
fn root_overflow_uses_viewport_scroll_and_percentage_basis() {
    let mut h = Harness::from_html(
        "<html style='overflow:auto'><body style='margin:0'><div id=t style='position:sticky;top:10%;height:20px'></div><div style='height:2000px'></div></body></html>",
    );
    let root = h.base().root_element().id;
    h.base_mut()
        .scroll_to(root, 0.0, 100.0, ScrollBehavior::Instant);
    assert_eq!(shift(&h, "#t"), 160.0);
    let t = h.node("#t");
    assert_eq!(h.base().get_client_bounding_rect(t).unwrap().y, 60.0);
}

#[test]
fn oversized_bottom_only_reduces_effective_end_inset() {
    let mut h = Harness::from_html(
        "<body style='margin:0'><div id=s style='height:100px;overflow:auto'><div style='height:1000px'><div style='height:200px'></div><div id=t style='position:sticky;bottom:0;height:200px'></div></div></div>",
    );
    scroll(&mut h, "#s", 150.0);
    assert_eq!(shift(&h, "#t"), -50.0);
}

#[test]
fn scroll_into_view_uses_stuck_position() {
    let mut h = Harness::from_html(
        "<body style='margin:0'><div id=t style='position:sticky;top:0;height:20px'><div id=child style='height:10px'></div></div><div style='height:2000px'></div></body>",
    );
    for selector in ["#t", "#child"] {
        h.base_mut()
            .set_viewport_scroll(blitz_dom::util::Point { x: 0.0, y: 300.0 });
        let target = h.node(selector);
        h.base_mut().scroll_into_view(
            target,
            ScrollBehavior::Instant,
            ScrollLogicalPosition::Nearest,
            ScrollLogicalPosition::Nearest,
        );
        assert_eq!(h.base().viewport_scroll().y, 300.0);
    }
}

#[test]
fn nested_sticky_table_parts_apply_each_offset_once() {
    let mut h = Harness::from_html(
        "<body style='margin:0'><div id=s style='height:100px;overflow:auto'><table style='width:200px;border-spacing:0'><thead id=g style='position:sticky;top:0'><tr id=r style='position:sticky;top:10px'><th id=t style='position:sticky;top:20px;height:60px;padding:0;vertical-align:top'><div id=child style='position:sticky;top:30px;height:10px'></div></th></tr></thead><tbody><tr><td style='height:500px;padding:0'></td></tr></tbody></table></div>",
    );
    scroll(&mut h, "#s", 100.0);
    assert_eq!(
        h.base().get_node(h.node("#s")).unwrap().scroll_offset().y,
        100.0
    );
    assert_eq!(shift(&h, "#g"), 100.0);
    assert_eq!(shift(&h, "#r"), 10.0);
    assert_eq!(shift(&h, "#t"), 120.0);
    let child = h.node("#child");
    assert_eq!(h.base().get_client_bounding_rect(child).unwrap().y, 30.0);
    set_style(&mut h, "#g", "position:static");
    assert_eq!(shift(&h, "#t"), 120.0);
    set_style(&mut h, "#r", "position:static");
    assert_eq!(shift(&h, "#t"), 120.0);
    set_style(
        &mut h,
        "#t",
        "position:static;height:60px;padding:0;vertical-align:top",
    );
    assert_eq!(shift(&h, "#t"), 0.0);
}

#[test]
fn inline_sticky_moves_geometry_paint_hits_and_selection_together() {
    let mut h = Harness::from_html(
        "<body style='margin:0'><div id=s style='height:100px;overflow:hidden'><div id=p style='height:1000px;font-size:20px;line-height:24px'><span id=t style='position:sticky;top:0;background:red;text-decoration:underline'>sticky<b id=n style='background:red'>nested</b><span id=box style='display:inline-block;width:20px;height:20px;background:blue'></span></span></div></div></body>",
    );
    let t = h.node("#t");
    let nested = h.node("#n");
    let p = h.node("#p");
    let before = h.base().node_client_rects(t);
    assert!(!before.is_empty() && before[0].width > 40.0);
    let hit_before = h.base().find_text_position(2.0, 12.0).unwrap();
    h.base_mut().set_text_selection(p, 0, p, 6);
    let before_paint = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, &mut h.base_mut(), 1.0, 300, 100, 0, 0),
        300,
        100,
    );
    scroll(&mut h, "#s", 100.0);
    let after = h.base().node_client_rects(t);
    assert_eq!(after.len(), before.len());
    for (before, after) in before.iter().zip(&after) {
        assert_eq!(
            (after.x, after.y, after.width, after.height),
            (before.x, before.y, before.width, before.height)
        );
    }
    assert_eq!(h.hit_node(2.0, 12.0), t);
    assert_eq!(h.base().find_text_position(2.0, 12.0).unwrap(), hit_before);
    assert_eq!(shift(&h, "#n"), 100.0);
    assert_eq!(shift(&h, "#box"), 100.0);
    let nested_rect = h.base().get_client_bounding_rect(nested).unwrap();
    assert!(nested_rect.y >= 0.0 && nested_rect.y < 24.0);
    let after_paint = render_to_buffer::<VelloCpuImageRenderer, _>(
        |scene| paint_scene(scene, &mut h.base_mut(), 1.0, 300, 100, 0, 0),
        300,
        100,
    );
    assert!(
        before_paint
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[..3] == [255, 0, 0])
    );
    assert!(
        before_paint == after_paint,
        "sticky glyphs, decorations, backgrounds, selection, and atomic children must stay in place"
    );
    set_style(&mut h, "#t", "position:static");
    assert_eq!(shift(&h, "#n"), 0.0);
    assert_eq!(shift(&h, "#box"), 0.0);
    assert!(h.base().get_client_bounding_rect(t).unwrap().y < 0.0);
}

#[test]
fn nested_inline_stickies_use_fragment_origins_and_ancestor_offsets() {
    let mut h = Harness::from_html(
        "<body style='margin:0'><div id=s style='height:100px;overflow:auto'><div style='height:1000px;font-size:20px;line-height:24px'><span id=outer style='position:sticky;top:0'>outer <span id=inner style='position:sticky;top:10px'>inner</span></span> sibling</div></div></body>",
    );
    scroll(&mut h, "#s", 100.0);
    let inner = h.node("#inner");
    assert_eq!(shift(&h, "#outer"), 100.0);
    assert_eq!(shift(&h, "#inner"), 110.0);
    assert_eq!(h.base().get_client_bounding_rect(inner).unwrap().y, 10.0);
}

#[test]
fn wrapped_inline_sticky_uses_its_in_flow_fragment_bounds() {
    let mut h = Harness::from_html(
        "<body style='margin:0'><div id=s style='height:100px;overflow:auto'><div style='height:1000px;width:100px;font-size:20px;line-height:24px'>prefix<br>prefix<br><span id=t style='position:sticky;top:0'>sticky text that wraps across several lines</span></div></div>",
    );
    let t = h.node("#t");
    let before = h.base().node_client_rects(t);
    assert!(before.len() > 1 && before[0].y > 0.0);
    scroll(&mut h, "#s", 200.0);
    let after = h.base().node_client_rects(t);
    assert_eq!(after.len(), before.len());
    assert_eq!(after[0].y, 0.0);
    for (before_line, after_line) in before.iter().zip(after) {
        assert_eq!(after_line.y, before_line.y - before[0].y);
    }
}

#[test]
fn resolve_after_dropping_sticky_during_smooth_scroll_does_not_panic() {
    let mut h = Harness::from_html(STICKY_HEADER);
    let t = h.node("#t");
    let root = h.base().root_element().id;
    h.base_mut()
        .scroll_to(root, 0.0, 100.0, ScrollBehavior::Smooth);
    h.base_mut().mutate().remove_and_drop_node(t);
    h.pump();
}

#[test]
fn scroll_after_dropping_cached_container_does_not_panic() {
    let mut h = Harness::from_html(
        "<body style='margin:0'><div id=s style='height:100px;overflow:auto'><div id=t style='position:sticky;top:0;height:20px'></div><div style='height:1000px'></div></div><div id=dest style='height:2000px'></div></body>",
    );
    let t = h.node("#t");
    let dest = h.node("#dest");
    let s = h.node("#s");
    h.base_mut().mutate().append_children(dest, &[t]);
    h.base_mut().mutate().remove_and_drop_node(s);
    h.base_mut()
        .set_viewport_scroll(blitz_dom::util::Point { x: 0.0, y: 100.0 });
    h.pump();
    assert_eq!(h.base().get_node(t).unwrap().sticky_offset().y, 100.0);
}
