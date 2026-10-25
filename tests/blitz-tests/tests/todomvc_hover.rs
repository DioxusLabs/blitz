use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use blitz_traits::shell::{ColorScheme, Viewport};

fn positions(doc: &HtmlDocument) -> Vec<(String, f32, f32, f32, f32)> {
    [
        ".todoapp",
        ".footer",
        ".filters",
        ".filters li",
        "#all",
        "#active",
        "#completed",
    ]
    .into_iter()
    .map(|selector| {
        let node = doc
            .get_node(doc.query_selector(selector).unwrap().unwrap())
            .unwrap();
        let pos = node.absolute_position(0.0, 0.0);
        let size = node.final_layout().size;
        (selector.to_owned(), pos.x, pos.y, size.width, size.height)
    })
    .collect()
}

#[test]
fn todomvc_hover_preserves_filter_positions() {
    let css = include_str!("../../../examples/todomvc/src/todomvc.css");
    let html = format!(
        r##"<!doctype html><html><head><style>{css}</style></head><body>
        <section class="todoapp"><header><input class="new-todo" placeholder="What needs to be done?"></header>
        <section class="main"><ul class="todo-list"><li><div class="view"><label>Test todo</label></div></li></ul></section>
        <footer class="footer"><span class="todo-count"><strong>1 </strong><span>item left</span></span>
        <ul class="filters"><li><a id="all" href="#/" class="selected">All</a></li><li><a id="active" href="#/active">Active</a></li><li><a id="completed" href="#/completed">Completed</a></li></ul></footer></section>
        </body></html>"##
    );
    for incremental in [false, true] {
        let mut doc = HtmlDocument::from_html(
            &html,
            DocumentConfig {
                viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
                incremental: Some(incremental),
                ..Default::default()
            },
        );
        doc.resolve(0.0);
        let initial = positions(&doc);
        println!("incremental={incremental} initial={initial:?}");
        for selector in ["#all", "#active", "#completed"] {
            let target = doc.query_selector(selector).unwrap().unwrap();
            let pos = doc.get_node(target).unwrap().absolute_position(2.0, 2.0);
            doc.set_hover_to(pos.x, pos.y);
            assert_eq!(doc.get_hover_node_id(), Some(target));
            doc.resolve(0.0);
            let after = positions(&doc);
            println!("incremental={incremental} hover={selector} after={after:?}");
            assert_eq!(initial, after);
        }
    }
}
