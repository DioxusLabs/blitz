// Example: dioxus + css + stylo
//
// Create a style context for a dioxus document.
use dioxus::prelude::*;

fn main() {
    let css = r#"
        h1 {
            background-color: red;
        }

        h2 {
            background-color: green;
        }

        h3 {
            background-color: blue;
        }

        h4 {
            background-color: yellow;
        }

        "#;

    let nodes = rsx! {
        h1 { }
        h2 { }
        h3 { }
        h4 { }
    };

    let document = blitz::Document::new();

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
