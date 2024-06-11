use html5ever::{local_name, namespace_url, ns, QualName};
use parley::{builder::TreeBuilder, style::TextStyle, InlineBox};
use slab::Slab;
use style::{
    properties::{longhands::line_height, ComputedValues},
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
                // dbg!(&doc.nodes[container_node_id].raw_dom_data);
                let (inline_layout, ilayout_children) = build_inline_layout(doc, container_node_id);
                doc.nodes[container_node_id].is_inline_root = true;
                doc.nodes[container_node_id].raw_dom_data.downcast_element_mut().unwrap().inline_layout = Some(Box::new(inline_layout));
                return layout_children.extend_from_slice(&ilayout_children);
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
                layout_children.push(node_id);
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

    // Convert text colour
    let [r, g, b, a] = itext_styles
        .color
        .to_color_space(style::color::ColorSpace::Srgb)
        .raw_components()
        .map(|f| (f * 255.0) as u8);
    let color = peniko::Color { r, g, b, a };

    // Parley expects line height as a multiple of font size!
    let line_height = line_height / font_size;

    TextStyle {
        font_stack: FontStack::Source("sans-serif"),
        font_size,
        font_stretch: Default::default(),
        font_style: Default::default(),
        font_weight: FontWeight::new(font_styles.font_weight.value()),
        font_variations: FontSettings::List(&[]),
        font_features: FontSettings::List(&[]),
        locale: Default::default(),
        brush: TextBrush { color },
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

#[derive(Debug, Clone, Copy)]
enum WhiteSpaceCollapse {
    Collapse,
    Preserve,
}

pub(crate) fn build_inline_layout(
    doc: &mut Document,
    inline_context_root_node_id: usize,
) -> (TextLayout, Vec<usize>) {
    // Get the inline context's root node's text styles
    let root_node = &doc.nodes[inline_context_root_node_id];
    let root_node_style = root_node.primary_styles();

    let parley_style = root_node_style
        .as_ref()
        .map(|s| stylo_to_parley_style(&*s))
        .unwrap_or_default();

    // TODO: Support more modes. For now we want to support enough for pre tags to render correctly
    let collapse_mode = root_node_style
        .map(|s| match s.get_inherited_text().white_space {
            style::computed_values::white_space::T::Normal => WhiteSpaceCollapse::Collapse,
            style::computed_values::white_space::T::Pre => WhiteSpaceCollapse::Preserve,
            style::computed_values::white_space::T::Nowrap => WhiteSpaceCollapse::Preserve,
            style::computed_values::white_space::T::PreWrap => WhiteSpaceCollapse::Preserve,
            style::computed_values::white_space::T::PreLine => WhiteSpaceCollapse::Preserve,
        })
        .unwrap_or(WhiteSpaceCollapse::Collapse);

    // Create a parley tree builder
    let mut text = String::new();
    let mut builder = doc
        .layout_ctx
        .tree_builder(&mut doc.font_ctx, 2.0, &parley_style);

    for child_id in root_node.children.iter().copied() {
        build_inline_layout_recursive(&mut text, &mut builder, &doc.nodes, child_id, collapse_mode);
    }

    let layout = builder.build(&text);

    // Obtain layout children for the inline layout
    let layout_children: Vec<usize> = layout
        .inline_boxes()
        .iter()
        .map(|ibox| ibox.id as usize)
        .collect();

    // Recurse into inline boxes within layout
    for child_id in layout_children.iter().copied() {
        doc.ensure_layout_children(child_id);
    }

    return (TextLayout { text, layout }, layout_children);

    fn build_inline_layout_recursive(
        text: &mut String,
        builder: &mut TreeBuilder<TextBrush>,
        nodes: &Slab<Node>,
        node_id: usize,
        collapse_mode: WhiteSpaceCollapse,
    ) {
        let node = &nodes[node_id];

        let collapse_mode = node
            .primary_styles()
            .map(|s| match s.get_inherited_text().white_space {
                style::computed_values::white_space::T::Normal => WhiteSpaceCollapse::Collapse,
                style::computed_values::white_space::T::Pre => WhiteSpaceCollapse::Preserve,
                style::computed_values::white_space::T::Nowrap => WhiteSpaceCollapse::Preserve,
                style::computed_values::white_space::T::PreWrap => WhiteSpaceCollapse::Preserve,
                style::computed_values::white_space::T::PreLine => WhiteSpaceCollapse::Preserve,
            })
            .unwrap_or(collapse_mode);

        match &node.raw_dom_data {
            NodeData::Element(element_data) | NodeData::AnonymousBlock(element_data) => {
                // Hide hidden nodes
                if let Some("hidden" | "") = element_data.attr(local_name!("hidden")) {
                    return;
                }

                // if the input type is hidden, hide it
                if *element_data.name.local == *"input" {
                    if let Some("hidden") = element_data.attr(local_name!("type")) {
                        return;
                    }
                }

                let display = node.display_style().unwrap_or(Display::inline());

                match display.inside() {
                    DisplayInside::None => return,
                    DisplayInside::Contents => {
                        for child_id in node.children.iter().copied() {
                            build_inline_layout_recursive(
                                text,
                                builder,
                                nodes,
                                child_id,
                                collapse_mode,
                            );
                        }
                    }
                    DisplayInside::Flow => {
                        if node
                            .raw_dom_data
                            .is_element_with_tag_name(&local_name!("img"))
                        {
                            builder.push_inline_box(InlineBox {
                                id: node_id as u64,
                                index: text.len(),
                                // Width and height are set during layout
                                width: 0.0,
                                height: 0.0,
                            });
                        } else {
                            let style = node
                                .primary_styles()
                                .map(|s| stylo_to_parley_style(&*s))
                                .unwrap_or_default();
                            builder.push_style_span(style);

                            for child_id in node.children.iter().copied() {
                                build_inline_layout_recursive(
                                    text,
                                    builder,
                                    nodes,
                                    child_id,
                                    collapse_mode,
                                );
                            }

                            builder.pop_style_span();
                        }
                    }
                    // Inline box
                    _ => {
                        builder.push_inline_box(InlineBox {
                            id: node_id as u64,
                            index: text.len(),
                            // Width and height are set during layout
                            width: 0.0,
                            height: 0.0,
                        });
                    }
                };
            }
            NodeData::Text(data) => {
                // if data.content.chars().all(|c| c.is_ascii_whitespace()) {
                //     text.push_str(" ");
                //     builder.push_text(1);
                // } else {
                match collapse_mode {
                    WhiteSpaceCollapse::Collapse => {
                        // Convert newlines to spaces
                        let new_text = data.content.replace("\n", " ");

                        // Completely remove leading whitespace
                        // TODO: should be per-span not per-inline-context
                        // TODO: should also trim trailing whitespace
                        let new_text = if text.len() == 0 {
                            new_text.trim_start()
                        } else {
                            &new_text
                        };

                        // Collapse spaces
                        let mut last_char_space = false;
                        let new_text: String = new_text
                            .chars()
                            .filter(|c| {
                                let this_char_space = c.is_ascii_whitespace();
                                let prev_char_space = last_char_space;
                                last_char_space = this_char_space;

                                !(prev_char_space && this_char_space)
                            })
                            .collect();

                        text.push_str(&new_text);
                        builder.push_text(new_text.len());
                    }
                    WhiteSpaceCollapse::Preserve => {
                        text.push_str(&data.content);
                        builder.push_text(data.content.len());
                    }
                }

                // }
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
