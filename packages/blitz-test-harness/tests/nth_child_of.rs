//! Tests for `:nth-child(An+B of S)` matching and invalidation.

use blitz_dom::qual_name;
use blitz_dom::{BaseDocument, LocalName, QualName, ns};
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
fn nth_child_of_matches_statically() {
    let harness = Harness::from_html(
        r#"
        <style>
            li { color: rgb(0, 0, 0); }
            li:nth-child(2 of .a) { color: rgb(255, 0, 0); }
        </style>
        <ul>
            <li class="a">one</li>
            <li>two</li>
            <li class="a">three</li>
            <li class="a">four</li>
        </ul>
        "#,
    );
    assert_eq!(colors(&harness, "li"), [BLACK, BLACK, RED, BLACK]);
}

#[test]
fn nth_child_of_invalidates_on_sibling_class_change() {
    let mut harness = Harness::from_html(
        r#"
        <style>
            li { color: rgb(0, 0, 0); }
            li:nth-child(2 of .a) { color: rgb(255, 0, 0); }
        </style>
        <ul>
            <li class="a">one</li>
            <li>two</li>
            <li class="a">three</li>
            <li class="a">four</li>
        </ul>
        "#,
    );
    assert_eq!(colors(&harness, "li"), [BLACK, BLACK, RED, BLACK]);

    // Removing .a from the first sibling makes "four" the second .a element.
    let first_li = harness.node("li");
    harness
        .base_mut()
        .mutate()
        .clear_attribute(first_li, qual_name!("class"));
    harness.pump();
    assert_eq!(colors(&harness, "li"), [BLACK, BLACK, BLACK, RED]);

    // Adding .a to the second sibling makes "three" the second .a element again.
    let second_li = harness.query_all("li")[1];
    harness
        .base_mut()
        .mutate()
        .set_attribute(second_li, qual_name!("class"), "a");
    harness.pump();
    assert_eq!(colors(&harness, "li"), [BLACK, BLACK, RED, BLACK]);
}

#[test]
fn nth_last_child_of_invalidates_on_sibling_class_change() {
    let mut harness = Harness::from_html(
        r#"
        <style>
            li { color: rgb(0, 0, 0); }
            li:nth-last-child(1 of .a) { color: rgb(255, 0, 0); }
        </style>
        <ul>
            <li class="a">one</li>
            <li class="a">two</li>
            <li>three</li>
        </ul>
        "#,
    );
    assert_eq!(colors(&harness, "li"), [BLACK, RED, BLACK]);

    // Removing .a from "two" makes "one" the last .a element: an *earlier*
    // sibling must be restyled.
    let second_li = harness.query_all("li")[1];
    harness
        .base_mut()
        .mutate()
        .clear_attribute(second_li, qual_name!("class"));
    harness.pump();
    assert_eq!(colors(&harness, "li"), [RED, BLACK, BLACK]);
}

#[test]
fn nth_child_of_invalidates_on_sibling_id_change() {
    let mut harness = Harness::from_html(
        r#"
        <style>
            li { color: rgb(0, 0, 0); }
            li:nth-child(1 of #foo) { color: rgb(255, 0, 0); }
        </style>
        <ul>
            <li>one</li>
            <li id="foo">two</li>
        </ul>
        "#,
    );
    assert_eq!(colors(&harness, "li"), [BLACK, RED]);

    let first_li = harness.node("li");
    harness
        .base_mut()
        .mutate()
        .set_attribute(first_li, qual_name!("id"), "foo");
    harness.pump();
    assert_eq!(colors(&harness, "li"), [RED, BLACK]);
}

#[test]
fn nth_child_of_invalidates_on_sibling_attribute_change() {
    let mut harness = Harness::from_html(
        r#"
        <style>
            li { color: rgb(0, 0, 0); }
            li:nth-child(2 of [data-x]) { color: rgb(255, 0, 0); }
        </style>
        <ul>
            <li data-x>one</li>
            <li>two</li>
            <li data-x>three</li>
        </ul>
        "#,
    );
    assert_eq!(colors(&harness, "li"), [BLACK, BLACK, RED]);

    let first_li = harness.node("li");
    harness.base_mut().mutate().clear_attribute(
        first_li,
        QualName::new(None, ns!(), LocalName::from("data-x")),
    );
    harness.pump();
    assert_eq!(colors(&harness, "li"), [BLACK, BLACK, BLACK]);
}

#[test]
fn nth_child_of_invalidates_on_sibling_state_change() {
    let mut harness = Harness::from_html(
        r#"
        <style>
            input { color: rgb(0, 0, 0); }
            input:nth-child(2 of :checked) { color: rgb(255, 0, 0); }
        </style>
        <div>
            <input type="checkbox" checked>
            <input type="checkbox">
            <input type="checkbox" checked>
        </div>
        "#,
    );
    assert_eq!(colors(&harness, "input"), [BLACK, BLACK, RED]);

    // Checking the second checkbox makes it the second :checked element.
    harness.click("input:nth-child(2)");
    harness.pump();
    assert_eq!(colors(&harness, "input"), [BLACK, RED, BLACK]);
}

#[test]
fn nth_child_of_invalidates_on_sibling_insertion_and_removal() {
    let mut harness = Harness::from_html(
        r#"
        <style>
            li { color: rgb(0, 0, 0); }
            li:nth-child(2 of .a) { color: rgb(255, 0, 0); }
        </style>
        <ul>
            <li class="a">one</li>
            <li class="a">two</li>
        </ul>
        "#,
    );
    assert_eq!(colors(&harness, "li"), [BLACK, RED]);

    let first_li = harness.node("li");
    {
        let mut doc = harness.base_mut();
        let mut mutator = doc.mutate();
        let new_li = mutator.create_element(qual_name!("li", html), Vec::new());
        mutator.set_attribute(new_li, qual_name!("class"), "a");
        mutator.insert_nodes_before(first_li, &[new_li]);
    }
    harness.pump();
    assert_eq!(colors(&harness, "li"), [BLACK, RED, BLACK]);

    // Removing the inserted element restores the original matching.
    let inserted_li = harness.node("li");
    harness.base_mut().mutate().remove_node(inserted_li);
    harness.pump();
    assert_eq!(colors(&harness, "li"), [BLACK, RED]);
}
