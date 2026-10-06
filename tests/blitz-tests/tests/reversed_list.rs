//! `reversed` is a boolean attribute: its bare form must reverse an ordered
//! list's numbering. Parsing its value would read `<ol reversed>` as not
//! reversed, since the empty string is not a boolean literal.

use blitz_dom::node::Marker;
use blitz_test_harness::Harness;

fn marker(harness: &Harness, selector: &str) -> String {
    let node_id = harness.node(selector);
    let doc = harness.base();
    let element = doc.get_node(node_id).unwrap().element_data().unwrap();
    match &element.list_item_data.as_ref().unwrap().marker {
        Marker::String(s) => s.clone(),
        Marker::Char(c) => c.to_string(),
    }
}

#[test]
fn a_bare_reversed_attribute_reverses_the_numbering() {
    let harness = Harness::from_html(
        "<html><body>\
         <ol reversed><li id=first>a</li><li>b</li><li id=last>c</li></ol>\
         </body></html>",
    );
    assert_eq!(marker(&harness, "#first"), "3. ");
    assert_eq!(marker(&harness, "#last"), "1. ");
}
