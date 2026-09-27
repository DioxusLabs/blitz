use blitz_dom::ScrollBehavior;
use blitz_test_harness::Harness;

const STICKY_HEADER: &str = "<body style='margin:0'><div id=t style='position:sticky;top:0;height:20px'></div><div style='height:2000px'></div></body>";

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
