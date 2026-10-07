use blitz_dom::node::Marker;
use blitz_test_harness::Harness;

fn assert_markers(html: &str, expected: &[&str]) {
    let harness = Harness::from_html(html);
    let doc = harness.base();
    let items = doc.query_selector_all(".item").unwrap();
    assert_eq!(items.len(), expected.len());
    for (id, expected) in items.into_iter().zip(expected) {
        let marker = &doc
            .get_node(id)
            .unwrap()
            .element_data()
            .unwrap()
            .list_item_data
            .as_ref()
            .unwrap()
            .marker;
        assert_eq!(marker, &Marker::String((*expected).to_string()));
    }
}

#[test]
fn decimal_list_items_outside_an_ordered_list_start_at_one() {
    assert_markers(
        r#"<style>.item { display: list-item; list-style: decimal inside; }</style>
        <div class="item">One</div><div class="item">Two</div>"#,
        &["1. ", "2. "],
    );
}

#[test]
fn alphabetical_list_items_outside_an_ordered_list_start_at_a() {
    assert_markers(
        r#"<style>.item { display: list-item; list-style: lower-alpha inside; }</style>
        <div class="item">One</div><div class="item">Two</div>"#,
        &["a. ", "b. "],
    );
}

#[test]
fn ordered_lists_keep_their_default_and_explicit_start() {
    assert_markers(
        r#"<ol><li class="item">One</li><li class="item">Two</li></ol>"#,
        &["1. ", "2. "],
    );
    assert_markers(
        r#"<ol start="4"><li class="item">Four</li><li class="item">Five</li></ol>"#,
        &["4. ", "5. "],
    );
}
