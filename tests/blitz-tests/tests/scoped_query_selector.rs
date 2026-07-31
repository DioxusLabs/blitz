use blitz_dom::{DocumentConfig, NodeData, NodeId};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn document() -> HtmlDocument {
    HtmlDocument::from_html(
        r#"
        <html>
            <body>
                <div id="a">
                    <div id="b" class="target" data-scope="yes">
                        <div id="c" class="target" data-match="yes">
                            <span id="d" class="target" data-match="yes"></span>
                        </div>
                        <div id="e"></div>
                    </div>
                    <div id="sibling" class="target" data-match="yes"></div>
                    <div id="text-scope">text<!-- comment --><span id="inside"></span></div>
                </div>
            </body>
        </html>
        "#,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    )
}

fn id(doc: &HtmlDocument, selector: &str) -> NodeId {
    doc.query_selector(selector)
        .unwrap()
        .unwrap_or_else(|| panic!("{selector} not found"))
}

#[test]
fn scoped_queries_exclude_scope_and_outside_nodes() {
    let doc = document();
    let scope = id(&doc, "#b");
    let c = id(&doc, "#c");
    let d = id(&doc, "#d");
    let e = id(&doc, "#e");

    assert_eq!(doc.query_selector_in(scope, ".target").unwrap(), Some(c));
    assert_eq!(
        doc.query_selector_all_in(scope, ".target")
            .unwrap()
            .as_slice(),
        &[c, d]
    );
    assert_eq!(
        doc.query_selector_all_in(scope, "#a").unwrap().as_slice(),
        &[]
    );
    assert_eq!(
        doc.query_selector_all_in(scope, "#sibling")
            .unwrap()
            .as_slice(),
        &[]
    );
    assert_eq!(
        doc.query_selector_all_in(scope, "div").unwrap().as_slice(),
        &[c, e]
    );
}

#[test]
fn scoped_selectors_can_match_ancestors_while_returning_descendants() {
    let doc = document();
    let scope = id(&doc, "#b");
    let c = id(&doc, "#c");
    let e = id(&doc, "#e");

    assert_eq!(
        doc.query_selector_all_in(scope, "#a div")
            .unwrap()
            .as_slice(),
        &[c, e]
    );
}

#[test]
fn scoped_query_all_covers_fast_and_slow_paths_in_tree_order() {
    let doc = document();
    let scope = id(&doc, "#b");
    let c = id(&doc, "#c");
    let d = id(&doc, "#d");
    let e = id(&doc, "#e");

    assert_eq!(
        doc.query_selector_all_in(scope, ".target")
            .unwrap()
            .as_slice(),
        &[c, d]
    );
    assert_eq!(
        doc.query_selector_all_in(scope, "div").unwrap().as_slice(),
        &[c, e]
    );
    assert_eq!(
        doc.query_selector_all_in(scope, "[data-match]")
            .unwrap()
            .as_slice(),
        &[c, d]
    );
    assert_eq!(
        doc.query_selector_all_in(scope, "#c").unwrap().as_slice(),
        &[c]
    );
    assert_eq!(
        doc.query_selector_all_in(scope, "div > span")
            .unwrap()
            .as_slice(),
        &[d]
    );
    assert_eq!(
        doc.query_selector_all_in(scope, ":is(div > span)")
            .unwrap()
            .as_slice(),
        &[d]
    );
}

#[test]
fn non_element_scopes_return_no_matches() {
    let doc = document();
    let text_scope = id(&doc, "#text-scope");
    let text = doc.get_node(text_scope).unwrap().children[0];
    let comment = doc
        .get_node(text_scope)
        .unwrap()
        .children
        .iter()
        .copied()
        .find(|node_id| {
            matches!(
                doc.get_node(*node_id).unwrap().data,
                NodeData::Comment { .. }
            )
        })
        .expect("comment not found");

    assert!(doc.query_selector_in(text, "span").unwrap().is_none());
    assert!(doc.query_selector_all_in(text, "*").unwrap().is_empty());
    assert!(doc.query_selector_in(comment, "span").unwrap().is_none());
    assert!(doc.query_selector_all_in(comment, "*").unwrap().is_empty());
}

#[test]
fn matches_and_closest_include_self_then_ancestors() {
    let doc = document();
    let b = id(&doc, "#b");
    let c = id(&doc, "#c");
    let d = id(&doc, "#d");

    assert!(doc.matches_selector(c, ".target").unwrap());
    assert!(!doc.matches_selector(d, "#c").unwrap());
    assert_eq!(doc.closest(c, "#c").unwrap(), Some(c));
    assert_eq!(doc.closest(d, "#b").unwrap(), Some(b));
    assert_eq!(doc.closest(c, "#a").unwrap(), Some(id(&doc, "#a")));
    assert_eq!(doc.closest(c, "#missing").unwrap(), None);

    let parsed = doc.try_parse_selector_list(".target").unwrap();
    assert_eq!(
        doc.get_node(c)
            .unwrap()
            .query_selector_all_raw(&parsed)
            .as_slice(),
        &[d]
    );
    assert!(doc.get_node(c).unwrap().matches_selector_raw(&parsed));
    assert_eq!(doc.get_node(c).unwrap().closest_raw(&parsed), Some(c));
}
