//! A `display: none` containing block must not keep the out-of-flow boxes from its
//! last visible layout in its hoisted child list. Rounding visits that list, so it
//! used to reach boxes which had since lost their styles or been removed, and panic
//! (issue #1560).

use blitz_dom::{DocumentConfig, qual_name};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn doc(html: &str) -> HtmlDocument {
    HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            ..Default::default()
        },
    )
}

const PSEUDO_HTML: &str = r#"<!doctype html><style>
    #panel { position: fixed; width: 260px }
    .outline::before, .outline::after { content: ''; position: absolute; inset: -10px }
</style><div id="panel" class="outline"><div>description</div></div>"#;

fn remove_pseudos_from_hidden_host(incremental: bool) {
    let mut doc = doc(PSEUDO_HTML);
    doc.set_incremental_layout(incremental);
    doc.resolve(0.0);
    let panel = doc.query_selector("#panel").unwrap().unwrap();
    doc.mutate().set_attribute(panel, qual_name!("hidden"), "");
    doc.resolve(0.0);
    doc.mutate().set_attribute(panel, qual_name!("class"), "");
    doc.resolve(0.0);
}

#[test]
fn remove_pseudos_from_hidden_host_incremental() {
    remove_pseudos_from_hidden_host(true);
}

#[test]
fn remove_pseudos_from_hidden_host_full() {
    remove_pseudos_from_hidden_host(false);
}

#[test]
fn remove_abspos_child_from_hidden_host() {
    let mut doc = doc(
        r#"<!doctype html><div id="panel" style="position: fixed; width: 200px">
            <div id="child" style="position: absolute; inset: -10px"></div>
        </div>"#,
    );
    doc.resolve(0.0);
    let panel = doc.query_selector("#panel").unwrap().unwrap();
    let child = doc.query_selector("#child").unwrap().unwrap();
    doc.mutate().set_attribute(panel, qual_name!("hidden"), "");
    doc.resolve(0.0);
    doc.mutate().remove_and_drop_node(child);
    doc.resolve(0.0);
}
