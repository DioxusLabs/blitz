// Example: dioxus + css + stylo
//
// Create a style context for a dioxus document.
use dioxus::prelude::*;

fn root() -> Element {
    let css = r#"
        .scrollable {
            background-color: green;
            overflow: scroll;
            height: 200px;
        }

        .gap {
            height: 300px;
            margin: 8px;
            background: #11ff11;
            display: flex;
            align-items: center;
            color: white;
        }

        .not-scrollable {
            background-color: yellow;
            padding-top: 16px;
            padding-bottom: 16px;
        "#;

    rsx! {
        style { {css} }
        div { class: "not-scrollable", "Not scrollable" }
        div { class: "scrollable",
            div {
                "Scroll me"
            }
            div {
                class: "gap",
                "gap"
            }
            div {
                "Hello"
            }
        }
        div { class: "not-scrollable", "Not scrollable" }
    }
}

fn main() {
    dioxus_blitz::launch(root);

    // let document = blitz_dom::Document::new();

    // let styled_dom = blitz::style_lazy_nodes(css, nodes);

    // print_styles(&styled_dom);
}

// fn print_styles(markup: &blitz::RealDom) {
//     use style::dom::{TElement, TNode};

//     let root = markup.root_node();
//     for node in 0..markup.nodes.len() {
//         let Some(el) = root.with(node).as_element() else {
//             continue;
//         };

//         let data = el.borrow_data().unwrap();
//         let primary = data.styles.primary();
//         let bg_color = &primary.get_background().background_color;

//         println!(
//             "Styles for node {node_idx}:\n{:#?}",
//             bg_color,
//             node_idx = node
//         );
//     }
// }
