//! Tests that structural selectors (`:nth-child`, `:last-child`, `:empty`, ...)
//! are correctly invalidated when children are inserted or removed.

use blitz_dom::BaseDocument;
use blitz_dom::qual_name;
use blitz_html::HtmlDocument;
use blitz_test_harness::Harness;
use blitz_traits::node_id::NodeId;

const RED: [f32; 3] = [1.0, 0.0, 0.0];
const BLACK: [f32; 3] = [0.0, 0.0, 0.0];

fn color_of(doc: &BaseDocument, node_id: NodeId) -> [f32; 3] {
    let styles = doc.get_node(node_id).unwrap().primary_styles().unwrap();
    let color = styles.clone_color();
    [color.components.0, color.components.1, color.components.2]
}

fn colors(harness: &Harness<HtmlDocument>, selector: &str) -> Vec<[f32; 3]> {
    let doc = harness.base();
    harness
        .query_all(selector)
        .into_iter()
        .map(|id| color_of(&doc, id))
        .collect()
}

#[test]
fn nth_child_invalidates_on_sibling_insertion_and_removal() {
    let mut harness = Harness::from_html(
        r#"
        <style>
            li { color: rgb(0, 0, 0); }
            li:nth-child(2) { color: rgb(255, 0, 0); }
        </style>
        <ul>
            <li>one</li>
            <li>two</li>
        </ul>
        "#,
    );
    assert_eq!(colors(&harness, "li"), [BLACK, RED]);

    let first_li = harness.node("li");
    {
        let mut doc = harness.base_mut();
        let mut mutator = doc.mutate();
        let new_li = mutator.create_element(qual_name!("li", html), Vec::new());
        mutator.insert_nodes_before(first_li, &[new_li]);
    }
    harness.pump();
    assert_eq!(colors(&harness, "li"), [BLACK, RED, BLACK]);

    let inserted_li = harness.node("li");
    harness.base_mut().mutate().remove_node(inserted_li);
    harness.pump();
    assert_eq!(colors(&harness, "li"), [BLACK, RED]);
}

#[test]
fn nth_last_child_invalidates_on_sibling_append() {
    let mut harness = Harness::from_html(
        r#"
        <style>
            li { color: rgb(0, 0, 0); }
            li:nth-last-child(2) { color: rgb(255, 0, 0); }
        </style>
        <ul>
            <li>one</li>
            <li>two</li>
        </ul>
        "#,
    );
    assert_eq!(colors(&harness, "li"), [RED, BLACK]);

    // Appending shifts the nth-last-child index of *earlier* siblings.
    let ul = harness.node("ul");
    {
        let mut doc = harness.base_mut();
        let mut mutator = doc.mutate();
        let new_li = mutator.create_element(qual_name!("li", html), Vec::new());
        mutator.append_children(ul, &[new_li]);
    }
    harness.pump();
    assert_eq!(colors(&harness, "li"), [BLACK, RED, BLACK]);
}

#[test]
fn last_child_invalidates_on_sibling_append_and_removal() {
    let mut harness = Harness::from_html(
        r#"
        <style>
            li { color: rgb(0, 0, 0); }
            li:last-child { color: rgb(255, 0, 0); }
        </style>
        <ul>
            <li>one</li>
            <li>two</li>
        </ul>
        "#,
    );
    assert_eq!(colors(&harness, "li"), [BLACK, RED]);

    let ul = harness.node("ul");
    {
        let mut doc = harness.base_mut();
        let mut mutator = doc.mutate();
        let new_li = mutator.create_element(qual_name!("li", html), Vec::new());
        mutator.append_children(ul, &[new_li]);
    }
    harness.pump();
    assert_eq!(colors(&harness, "li"), [BLACK, BLACK, RED]);

    // Removing the new last child makes "two" the last child again.
    let last_li = *harness.query_all("li").last().unwrap();
    harness.base_mut().mutate().remove_node(last_li);
    harness.pump();
    assert_eq!(colors(&harness, "li"), [BLACK, RED]);
}

#[test]
fn moved_node_is_restyled_for_new_ancestors() {
    let mut harness = Harness::from_html(
        r#"
        <style>
            span { color: rgb(0, 0, 0); }
            .red span { color: rgb(255, 0, 0); }
        </style>
        <div id="a" class="red"><span>text</span></div>
        <div id="b"></div>
        "#,
    );
    assert_eq!(colors(&harness, "span"), [RED]);

    // Moving the span from #a to #b changes its ancestors, so it must be restyled.
    let span = harness.node("span");
    let b = harness.node("#b");
    harness.base_mut().mutate().append_children(b, &[span]);
    harness.pump();
    assert_eq!(colors(&harness, "span"), [BLACK]);

    // And back again.
    let a = harness.node("#a");
    harness.base_mut().mutate().append_children(a, &[span]);
    harness.pump();
    assert_eq!(colors(&harness, "span"), [RED]);
}

#[test]
fn empty_invalidates_on_child_insertion_and_removal() {
    let mut harness = Harness::from_html(
        r#"
        <style>
            div { color: rgb(0, 0, 0); }
            div:empty { color: rgb(255, 0, 0); }
        </style>
        <div id="container"></div>
        "#,
    );
    let container = harness.node("#container");
    assert_eq!(colors(&harness, "#container"), [RED]);

    let child = {
        let mut doc = harness.base_mut();
        let mut mutator = doc.mutate();
        let child = mutator.create_element(qual_name!("span", html), Vec::new());
        mutator.append_children(container, &[child]);
        child
    };
    harness.pump();
    assert_eq!(colors(&harness, "#container"), [BLACK]);

    harness.base_mut().mutate().remove_node(child);
    harness.pump();
    assert_eq!(colors(&harness, "#container"), [RED]);
}
