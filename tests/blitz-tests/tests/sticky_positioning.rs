use blitz_dom::{ScrollBehavior, ScrollLogicalPosition};
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
