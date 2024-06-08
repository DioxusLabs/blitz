use html5ever::{local_name, namespace_url, ns, QualName};
use parley::{builder::TreeBuilder, style::TextStyle};
use slab::Slab;
use style::{
    properties::ComputedValues,
    values::{
        computed::Display,
        specified::box_::{DisplayInside, DisplayOutside},
    },
};

use crate::{
    node::{NodeKind, TextBrush, TextLayout},
    Document, ElementNodeData, Node, NodeData,
};

pub(crate) fn collect_layout_children(
    doc: &mut Document,
    container_node_id: usize,
    layout_children: &mut Vec<usize>,
    anonymous_block_id: &mut Option<usize>,
) {
    if doc.nodes[container_node_id].children.is_empty() {
        return;
    }

    let container_display = doc.nodes[container_node_id]
        .display_style()
        .unwrap_or(Display::inline());

    match container_display.inside() {
        DisplayInside::None => {},
        DisplayInside::Contents => {

            // Take children array from node to avoid borrow checker issues.
            let children = std::mem::replace(&mut doc.nodes[container_node_id].children, Vec::new());

            for child_id in children.iter().copied() {
              collect_layout_children(doc, child_id, layout_children, anonymous_block_id)
            }

            // Put children array back
            doc.nodes[container_node_id].children = children;

        },
        DisplayInside::Flow | DisplayInside::FlowRoot => {

            // TODO: make "all_inline" detection work in the presence of display:contents nodes
            let mut all_block = true;
            let mut all_inline = true;
            let mut has_contents = false;
            for child in doc.nodes[container_node_id].children
                .iter()
                .copied()
                .map(|child_id| &doc.nodes[child_id])
            {
                // dbg!(child.raw_dom_data.kind());

                // Unwraps on Text and SVG nodes
                let display = child.display_style().unwrap_or(Display::inline());
                if matches!(display.inside(), DisplayInside::Contents) {
                    has_contents = true;
                } else {
                    match display.outside() {
                        DisplayOutside::None => {}
                        DisplayOutside::Inline => all_block = false,
                        DisplayOutside::Block => all_inline = false,

                        // TODO: Implement table layout
                        DisplayOutside::TableCaption => {}
                        DisplayOutside::InternalTable => {}
                    }
                }
            }

            // TODO: fix display:contents
            if all_inline {
                let inline_layout = build_inline_layout(doc, container_node_id);
                doc.nodes[container_node_id].is_inline_root = true;
                dbg!(&doc.nodes[container_node_id].raw_dom_data);
                doc.nodes[container_node_id].raw_dom_data.downcast_element_mut().unwrap().inline_layout = Some(Box::new(inline_layout));
                return layout_children.extend_from_slice(&doc.nodes[container_node_id].children);
            }

            // If the children are either all inline or all block then simply return the regular children
            // as the layout children
            if (all_block | all_inline) & !has_contents {
                return layout_children.extend_from_slice(&doc.nodes[container_node_id].children);
            }

            fn block_item_needs_wrap(child_node_kind: NodeKind, display_outside: DisplayOutside) -> bool {
                child_node_kind == NodeKind::Text || display_outside == DisplayOutside::Inline
            }
            collect_complex_layout_children(doc, container_node_id, layout_children, anonymous_block_id, block_item_needs_wrap);
        }
        DisplayInside::Flex /* | Display::Grid */ => {

            let has_text_node_or_contents = doc.nodes[container_node_id].children
                .iter()
                .copied()
                .map(|child_id| &doc.nodes[child_id])
                .any(|child| {
                    let display = child.display_style().unwrap_or(Display::inline());
                    let node_kind = child.raw_dom_data.kind();
                    display.inside() == DisplayInside::Contents || node_kind == NodeKind::Text
                });

            if !has_text_node_or_contents {
                return layout_children.extend_from_slice(&doc.nodes[container_node_id].children);
            }

            fn flex_or_grid_item_needs_wrap(child_node_kind: NodeKind, _display_outside: DisplayOutside) -> bool {
                child_node_kind == NodeKind::Text
            }
            collect_complex_layout_children(doc, container_node_id, layout_children, anonymous_block_id, flex_or_grid_item_needs_wrap);
        },

        // TODO: Implement table layout
        _ => {
            layout_children.extend_from_slice(&doc.nodes[container_node_id].children);
        }
    }
}

/// Handles the cases where there are text nodes or inline nodes that need to be wrapped in an anonymous block node
fn collect_complex_layout_children(
    doc: &mut Document,
    container_node_id: usize,
    layout_children: &mut Vec<usize>,
    anonymous_block_id: &mut Option<usize>,
    needs_wrap: impl Fn(NodeKind, DisplayOutside) -> bool,
) {
    // #[inline(always)]
    // fn needs_wrap(container_display_inside: DisplayInside, child_node_kind: NodeKind, child_display_outside: DisplayOutside) -> bool {
    //     child_node_kind == NodeKind::Text || (container_display_inside ==)
    // }

    // Take children array from node to avoid borrow checker issues.
    let children = std::mem::replace(&mut doc.nodes[container_node_id].children, Vec::new());

    for child_id in children.iter().copied() {
        // Get node kind (text, element, comment, etc)
        let child_node_kind = doc.nodes[child_id].raw_dom_data.kind();

        // Get Display style. Default to inline because nodes without styles are probably text nodes
        let child_display = &doc.nodes[child_id]
            .display_style()
            .unwrap_or(Display::inline());
        let display_inside = child_display.inside();
        let display_outside = child_display.outside();

        // Skip comment nodes. Note that we do *not* skip `Display::None` nodes as they may need to be hidden.
        // Taffy knows how to deal with `Display::None` children.
        if child_node_kind == NodeKind::Comment {
            continue;
        }
        // Recurse into `Display::Contents` nodes
        else if display_inside == DisplayInside::Contents {
            collect_layout_children(doc, child_id, layout_children, anonymous_block_id)
        }
        // Push nodes that need wrapping into the current "anonymous block container".
        // If there is not an open one then we create one.
        else if needs_wrap(child_node_kind, display_outside) {
            if anonymous_block_id.is_none() {
                const NAME: QualName = QualName {
                    prefix: None,
                    ns: ns!(html),
                    local: local_name!("div"),
                };
                let node_id = doc.create_node(NodeData::AnonymousBlock(ElementNodeData::new(
                    NAME,
                    Vec::new(),
                )));
                *anonymous_block_id = Some(node_id);
            }

            doc.nodes[anonymous_block_id.unwrap()]
                .children
                .push(child_id);
        }
        // Else push the child directly (and close any open "anonymous block container")
        else {
            *anonymous_block_id = None;
            layout_children.push(child_id);
        }
    }

    // Put children array back
    doc.nodes[container_node_id].children = children;
}

pub(crate) fn stylo_to_parley_style(style: &ComputedValues) -> TextStyle<'static, TextBrush> {
    use parley::style::*;

    let font_styles = style.get_font();
    // let text_styles = style.get_text();
    let itext_styles = style.get_inherited_text();

    let font_size = font_styles.font_size.used_size.0.px();
    let line_height: f32 = match itext_styles.line_height {
        style::values::generics::text::LineHeight::Normal => font_size * 1.2,
        style::values::generics::text::LineHeight::Number(num) => font_size * num.0,
        style::values::generics::text::LineHeight::Length(value) => value.0.px(),
    };

    TextStyle {
        font_stack: FontStack::Source("sans-serif"),
        font_size,
        font_stretch: Default::default(),
        font_style: Default::default(),
        font_weight: FontWeight::new(font_styles.font_weight.value()),
        font_variations: FontSettings::List(&[]),
        font_features: FontSettings::List(&[]),
        locale: Default::default(),
        brush: TextBrush,
        has_underline: Default::default(),
        underline_offset: Default::default(),
        underline_size: Default::default(),
        underline_brush: Default::default(),
        has_strikethrough: Default::default(),
        strikethrough_offset: Default::default(),
        strikethrough_size: Default::default(),
        strikethrough_brush: Default::default(),
        line_height,
        word_spacing: Default::default(),
        letter_spacing: Default::default(),
    }
}

pub(crate) fn build_inline_layout(
    doc: &mut Document,
    inline_context_root_node_id: usize,
) -> TextLayout {
    // Get the inline context's root node's text styles
    let root_node = &doc.nodes[inline_context_root_node_id];
    let root_node_style = root_node
        .primary_styles()
        .map(|s| stylo_to_parley_style(&*s))
        .unwrap_or_default();

    // Create a parley tree builder
    let mut text = String::new();
    let mut builder = doc
        .layout_ctx
        .tree_builder(&mut doc.font_ctx, 1.0, &root_node_style);

    for child_id in root_node.children.iter().copied() {
        build_inline_layout_recursive(&mut text, &mut builder, &doc.nodes, child_id);
    }

    let layout = builder.build(&text);
    return TextLayout { text, layout };

    fn build_inline_layout_recursive(
        text: &mut String,
        builder: &mut TreeBuilder<TextBrush>,
        nodes: &Slab<Node>,
        node_id: usize,
    ) {
        let node = &nodes[node_id];

        match &node.raw_dom_data {
            NodeData::Element(_) | NodeData::AnonymousBlock(_) => {
                let style = &*node.primary_styles().unwrap();
                let parley_style = stylo_to_parley_style(style);
                builder.push_style_span(parley_style);

                for child_id in node.children.iter().copied() {
                    build_inline_layout_recursive(text, builder, nodes, child_id);
                }
            }
            NodeData::Text(data) => {
                text.push_str(&data.content);
                builder.push_text(data.content.len())
            }
            NodeData::Comment => {}
            NodeData::Document => unreachable!(),
        }
    }
}

// pub (crate) fn determine_layout_children(
//     doc: &mut Document,
//     container_node_id: usize,
// ) -> Option<Cow<'_, [usize]>> {
//     let container_display = doc.nodes[container_node_id]
//         .display_style()
//         .unwrap_or(Display::inline());

//     match container_display.inside() {
//         DisplayInside::None => None,
//         DisplayInside::Contents => None, // Some(Cow::Borrowed(&container.children))},
//         DisplayInside::Flow | DisplayInside::FlowRoot => {
//             let mut all_block = true;
//             let mut all_inline = true;
//             let mut has_contents = false;
//             for child in doc.nodes[container_node_id].children
//                 .iter()
//                 .copied()
//                 .map(|child_id| &doc.nodes[child_id])
//             {
//                 let display = child.display_style().unwrap();
//                 match display.outside() {
//                     DisplayOutside::None => {}
//                     DisplayOutside::Inline => all_block = false,
//                     DisplayOutside::Block => all_inline = false,

//                     // TODO: Implement table layout
//                     DisplayOutside::TableCaption => {}
//                     DisplayOutside::InternalTable => {}
//                 }
//                 if matches!(display.inside(), DisplayInside::Contents) {
//                     has_contents = true;
//                 }
//             }

//             // If the children are either all inline
//             if (all_block | all_inline) & !has_contents {
//                 return None;
//             }

//             fn block_item_needs_wrap(child_node_kind: NodeKind, display_outside: DisplayOutside) -> bool {
//                 child_node_kind == NodeKind::Text || display_outside == DisplayOutside::Inline
//             }

//             let layout_children = collect_layout_children(doc, container_node_id, block_item_needs_wrap);
//             return Some(Cow::Owned(layout_children));
//         }
//         DisplayInside::Flex /* | Display::Grid */ => {
//             let has_text_node_or_contents = doc.nodes[container_node_id].children
//                 .iter()
//                 .copied()
//                 .map(|child_id| &doc.nodes[child_id])
//                 .any(|child| {
//                     let display = child.display_style().unwrap();
//                     let node_kind = child.raw_dom_data.kind();
//                     display.inside() == DisplayInside::Contents || node_kind == NodeKind::Text
//                 });

//             if !has_text_node_or_contents {
//                 return None;
//             }

//             fn flex_or_grid_item_needs_wrap(child_node_kind: NodeKind, _display_outside: DisplayOutside) -> bool {
//                 child_node_kind == NodeKind::Text
//             }

//             let layout_children = collect_layout_children(doc, container_node_id, flex_or_grid_item_needs_wrap);
//             return Some(Cow::Owned(layout_children));
//         },

//         // TODO: Implement table layout
//         DisplayInside::Table => None, //Some(Cow::Borrowed(&container.children)),
//         DisplayInside::TableRowGroup => None, //Some(Cow::Borrowed(&container.children)),
//         DisplayInside::TableColumn => None, //Some(Cow::Borrowed(&container.children)),
//         DisplayInside::TableColumnGroup => None, //Some(Cow::Borrowed(&container.children)),
//         DisplayInside::TableHeaderGroup => None, //Some(Cow::Borrowed(&container.children)),
//         DisplayInside::TableFooterGroup => None, //Some(Cow::Borrowed(&container.children)),
//         DisplayInside::TableRow => None, //Some(Cow::Borrowed(&container.children)),
//         DisplayInside::TableCell => None, //Some(Cow::Borrowed(&container.children)),
//     }
// }
