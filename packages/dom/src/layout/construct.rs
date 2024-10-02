use core::str;
use std::sync::Arc;

use html5ever::{local_name, namespace_url, ns, QualName};
use parley::{
    builder::TreeBuilder,
    style::{FontStack, WhiteSpaceCollapse},
    InlineBox,
};
use slab::Slab;
use style::{
    data::ElementData,
    properties::longhands::{
        list_style_position::computed_value::T as ListStylePosition,
        list_style_type::computed_value::T as ListStyleType,
    },
    shared_lock::StylesheetGuards,
    values::{
        computed::Display,
        specified::box_::{DisplayInside, DisplayOutside},
    },
};

use crate::{
    node::{
        ListItemLayout, ListItemLayoutPosition, Marker, NodeKind, NodeSpecificData, TextBrush,
        TextInputData, TextLayout,
    },
    stylo_to_parley, Document, ElementNodeData, Node, NodeData,
};

use super::table::build_table_context;

pub(crate) fn collect_layout_children(
    doc: &mut Document,
    container_node_id: usize,
    layout_children: &mut Vec<usize>,
    anonymous_block_id: &mut Option<usize>,
) {
    if let Some(el) = doc.nodes[container_node_id].raw_dom_data.downcast_element() {
        // Handle text inputs
        let tag_name = el.name.local.as_ref();
        if matches!(tag_name, "input" | "textarea") {
            let type_attr: Option<&str> = doc.nodes[container_node_id]
                .raw_dom_data
                .downcast_element()
                .and_then(|el| el.attr(local_name!("type")));
            if tag_name == "textarea" {
                create_text_editor(doc, container_node_id, true);
                return;
            } else if matches!(
                type_attr,
                Some("text" | "password" | "email" | "number" | "search" | "tel" | "url")
            ) {
                create_text_editor(doc, container_node_id, false);
                return;
            } else if type_attr == Some("checkbox") {
                create_checkbox_input(doc, container_node_id);
                return;
            }
        }

        //Only ol tags have start and reversed attributes
        let (mut index, reversed) = if tag_name == "ol" {
            (
                el.attr_parsed(local_name!("start"))
                    .map(|start: usize| start - 1)
                    .unwrap_or(0),
                el.attr_parsed(local_name!("reversed")).unwrap_or(false),
            )
        } else {
            (1, false)
        };
        collect_list_item_children(doc, &mut index, reversed, container_node_id);
    }

    if doc.nodes[container_node_id].children.is_empty() {
        return;
    }

    let container_display = doc.nodes[container_node_id].display_style().unwrap_or(
        match doc.nodes[container_node_id].raw_dom_data.kind() {
            NodeKind::AnonymousBlock => Display::Block,
            _ => Display::Inline,
        },
    );

    match container_display.inside() {
        DisplayInside::None => {}
        DisplayInside::Contents => {
            // Take children array from node to avoid borrow checker issues.
            let children = std::mem::take(&mut doc.nodes[container_node_id].children);

            for child_id in children.iter().copied() {
                collect_layout_children(doc, child_id, layout_children, anonymous_block_id)
            }

            // Put children array back
            doc.nodes[container_node_id].children = children;
        }
        DisplayInside::Flow | DisplayInside::FlowRoot | DisplayInside::TableCell => {
            // TODO: make "all_inline" detection work in the presence of display:contents nodes
            let mut all_block = true;
            let mut all_inline = true;
            let mut has_contents = false;
            for child in doc.nodes[container_node_id]
                .children
                .iter()
                .copied()
                .map(|child_id| &doc.nodes[child_id])
            {
                // Unwraps on Text and SVG nodes
                let display = child.display_style().unwrap_or(Display::inline());
                if matches!(display.inside(), DisplayInside::Contents) {
                    has_contents = true;
                } else {
                    match display.outside() {
                        DisplayOutside::None => {}
                        DisplayOutside::Block
                        | DisplayOutside::TableCaption
                        | DisplayOutside::InternalTable => all_inline = false,
                        DisplayOutside::Inline => {
                            all_block = false;

                            // We need the "complex" tree fixing when an inline contains a block
                            if child.is_or_contains_block() {
                                all_inline = false;
                            }
                        }
                    }
                }
            }

            // TODO: fix display:contents
            if all_inline {
                let (inline_layout, ilayout_children) = build_inline_layout(doc, container_node_id);
                doc.nodes[container_node_id].is_inline_root = true;
                doc.nodes[container_node_id]
                    .raw_dom_data
                    .downcast_element_mut()
                    .unwrap()
                    .inline_layout_data = Some(Box::new(inline_layout));
                return layout_children.extend_from_slice(&ilayout_children);
            }

            // If the children are either all inline or all block then simply return the regular children
            // as the layout children
            if (all_block | all_inline) & !has_contents {
                return layout_children.extend_from_slice(&doc.nodes[container_node_id].children);
            }

            fn block_item_needs_wrap(
                child_node_kind: NodeKind,
                display_outside: DisplayOutside,
            ) -> bool {
                child_node_kind == NodeKind::Text || display_outside == DisplayOutside::Inline
            }
            collect_complex_layout_children(
                doc,
                container_node_id,
                layout_children,
                anonymous_block_id,
                false,
                block_item_needs_wrap,
            );
        }
        DisplayInside::Flex | DisplayInside::Grid => {
            let has_text_node_or_contents = doc.nodes[container_node_id]
                .children
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

            fn flex_or_grid_item_needs_wrap(
                child_node_kind: NodeKind,
                _display_outside: DisplayOutside,
            ) -> bool {
                child_node_kind == NodeKind::Text
            }
            collect_complex_layout_children(
                doc,
                container_node_id,
                layout_children,
                anonymous_block_id,
                true,
                flex_or_grid_item_needs_wrap,
            );
        }

        DisplayInside::Table => {
            let (table_context, tlayout_children) = build_table_context(doc, container_node_id);
            doc.nodes[container_node_id].is_table_root = true;
            doc.nodes[container_node_id]
                .raw_dom_data
                .downcast_element_mut()
                .unwrap()
                .node_specific_data = NodeSpecificData::TableRoot(Arc::new(table_context));
            layout_children.extend_from_slice(&tlayout_children);
        }

        _ => {
            layout_children.extend_from_slice(&doc.nodes[container_node_id].children);
        }
    }
}

fn collect_list_item_children(
    doc: &mut Document,
    index: &mut usize,
    reversed: bool,
    node_id: usize,
) {
    let mut children = doc.nodes[node_id].children.clone();
    if reversed {
        children.reverse();
    }
    for child in children.into_iter() {
        if let Some(layout) = node_list_item_child(doc, child, *index) {
            let node = &mut doc.nodes[child];
            node.element_data_mut().unwrap().list_item_data = Some(Box::new(layout));
            *index += 1;
            collect_list_item_children(doc, index, reversed, child);
        } else {
            // Unset marker in case it was previously set
            let node = &mut doc.nodes[child];
            if let Some(element_data) = node.element_data_mut() {
                element_data.list_item_data = None;
            }
        }
    }
}

// Return a child node which is of display: list-item
fn node_list_item_child(doc: &mut Document, child: usize, index: usize) -> Option<ListItemLayout> {
    let node = &doc.nodes[child];

    // We only care about elements with display: list-item (li's have this automatically)
    if !node
        .primary_styles()
        .is_some_and(|style| style.get_box().display.is_list_item())
    {
        return None;
    }

    //Break on container elements when already in a list
    if node
        .element_data()
        .map(|element_data| {
            matches!(
                element_data.name.local,
                local_name!("ol") | local_name!("ul"),
            )
        })
        .unwrap_or(false)
    {
        return None;
    };

    let styles = node.primary_styles().unwrap();
    let list_style_type = styles.clone_list_style_type();
    let list_style_position = styles.clone_list_style_position();
    let marker = marker_for_style(list_style_type, index)?;

    let position = match list_style_position {
        ListStylePosition::Inside => ListItemLayoutPosition::Inside,
        ListStylePosition::Outside => {
            let mut parley_style = stylo_to_parley::style(&styles);

            if let Some(font_stack) = font_for_bullet_style(list_style_type) {
                parley_style.font_stack = font_stack;
            }

            // Create a parley tree builder
            let mut builder =
                doc.layout_ctx
                    .tree_builder(&mut doc.font_ctx, doc.viewport.scale(), &parley_style);

            match &marker {
                Marker::Char(char) => builder.push_text(&char.to_string()),
                Marker::String(str) => builder.push_text(str),
            };

            let mut layout = builder.build().0;

            layout.break_all_lines(Some(0.0));

            ListItemLayoutPosition::Outside(Box::new(layout))
        }
    };

    Some(ListItemLayout { marker, position })
}

// Determine the marker to render for a given list style type
fn marker_for_style(list_style_type: ListStyleType, index: usize) -> Option<Marker> {
    if list_style_type == ListStyleType::None {
        return None;
    }

    Some(match list_style_type {
        ListStyleType::LowerAlpha => {
            let mut marker = String::new();
            build_alpha_marker(index, &mut marker);
            Marker::String(format!("{}. ", marker))
        }
        ListStyleType::UpperAlpha => {
            let mut marker = String::new();
            build_alpha_marker(index, &mut marker);
            Marker::String(format!("{}. ", marker.to_ascii_uppercase()))
        }
        ListStyleType::Decimal => Marker::String(format!("{}. ", index + 1)),
        ListStyleType::Disc => Marker::Char('•'),
        ListStyleType::Circle => Marker::Char('◦'),
        ListStyleType::Square => Marker::Char('▪'),
        ListStyleType::DisclosureOpen => Marker::Char('▾'),
        ListStyleType::DisclosureClosed => Marker::Char('▸'),
        _ => Marker::Char('□'),
    })
}

// Override the font to our specific bullet font when rendering bullets
fn font_for_bullet_style(list_style_type: ListStyleType) -> Option<FontStack<'static>> {
    let bullet_font = Some(FontStack::Source("Bullet, monospace, sans-serif"));
    match list_style_type {
        ListStyleType::Disc
        | ListStyleType::Circle
        | ListStyleType::Square
        | ListStyleType::DisclosureOpen
        | ListStyleType::DisclosureClosed => bullet_font,
        _ => None,
    }
}

const ALPHABET: [char; 26] = [
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's',
    't', 'u', 'v', 'w', 'x', 'y', 'z',
];

// Construct alphanumeric marker from index, appending characters when index exceeds powers of 26
fn build_alpha_marker(index: usize, str: &mut String) {
    let rem = index % 26;
    let sym = ALPHABET[rem];
    str.insert(0, sym);
    let rest = (index - rem) as i64 / 26 - 1;
    if rest >= 0 {
        build_alpha_marker(rest as usize, str);
    }
}

#[test]
fn test_marker_for_disc() {
    let result = marker_for_style(ListStyleType::Disc, 0);
    assert_eq!(result, Some(Marker::Char('•')));
}

#[test]
fn test_marker_for_decimal() {
    let result_1 = marker_for_style(ListStyleType::Decimal, 0);
    let result_2 = marker_for_style(ListStyleType::Decimal, 1);
    assert_eq!(result_1, Some(Marker::String("1. ".to_string())));
    assert_eq!(result_2, Some(Marker::String("2. ".to_string())));
}

#[test]
fn test_marker_for_lower_alpha() {
    let result_1 = marker_for_style(ListStyleType::LowerAlpha, 0);
    let result_2 = marker_for_style(ListStyleType::LowerAlpha, 1);
    let result_extended_1 = marker_for_style(ListStyleType::LowerAlpha, 26);
    let result_extended_2 = marker_for_style(ListStyleType::LowerAlpha, 27);
    assert_eq!(result_1, Some(Marker::String("a. ".to_string())));
    assert_eq!(result_2, Some(Marker::String("b. ".to_string())));
    assert_eq!(result_extended_1, Some(Marker::String("aa. ".to_string())));
    assert_eq!(result_extended_2, Some(Marker::String("ab. ".to_string())));
}

#[test]
fn test_marker_for_upper_alpha() {
    let result_1 = marker_for_style(ListStyleType::UpperAlpha, 0);
    let result_2 = marker_for_style(ListStyleType::UpperAlpha, 1);
    let result_extended_1 = marker_for_style(ListStyleType::UpperAlpha, 26);
    let result_extended_2 = marker_for_style(ListStyleType::UpperAlpha, 27);
    assert_eq!(result_1, Some(Marker::String("A. ".to_string())));
    assert_eq!(result_2, Some(Marker::String("B. ".to_string())));
    assert_eq!(result_extended_1, Some(Marker::String("AA. ".to_string())));
    assert_eq!(result_extended_2, Some(Marker::String("AB. ".to_string())));
}

/// Handles the cases where there are text nodes or inline nodes that need to be wrapped in an anonymous block node
fn collect_complex_layout_children(
    doc: &mut Document,
    container_node_id: usize,
    layout_children: &mut Vec<usize>,
    anonymous_block_id: &mut Option<usize>,
    hide_whitespace: bool,
    needs_wrap: impl Fn(NodeKind, DisplayOutside) -> bool,
) {
    // Take children array from node to avoid borrow checker issues.
    let children = std::mem::take(&mut doc.nodes[container_node_id].children);

    for child_id in children.iter().copied() {
        // Get node kind (text, element, comment, etc)
        let child_node_kind = doc.nodes[child_id].raw_dom_data.kind();

        // Get Display style. Default to inline because nodes without styles are probably text nodes
        let contains_block = doc.nodes[child_id].is_or_contains_block();
        let child_display = &doc.nodes[child_id]
            .display_style()
            .unwrap_or(Display::inline());
        let display_inside = child_display.inside();
        let display_outside = if contains_block {
            DisplayOutside::Block
        } else {
            child_display.outside()
        };

        let is_whitespace_node = match &doc.nodes[child_id].raw_dom_data {
            NodeData::Text(data) => data.content.chars().all(|c| c.is_ascii_whitespace()),
            _ => false,
        };

        // Skip comment nodes. Note that we do *not* skip `Display::None` nodes as they may need to be hidden.
        // Taffy knows how to deal with `Display::None` children.
        //
        // Also hide all-whitespace flexbox children as these should be ignored
        if child_node_kind == NodeKind::Comment || (hide_whitespace && is_whitespace_node) {
            continue;
        }
        // Recurse into `Display::Contents` nodes
        else if display_inside == DisplayInside::Contents {
            collect_layout_children(doc, child_id, layout_children, anonymous_block_id)
        }
        // Push nodes that need wrapping into the current "anonymous block container".
        // If there is not an open one then we create one.
        else if needs_wrap(child_node_kind, display_outside) {
            use style::selector_parser::PseudoElement;

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

                // Set style data
                let parent_style = doc.nodes[container_node_id].primary_styles().unwrap();
                let read_guard = doc.guard.read();
                let guards = StylesheetGuards::same(&read_guard);
                let style = doc.stylist.style_for_anonymous::<&Node>(
                    &guards,
                    &PseudoElement::ServoAnonymousBox,
                    &parent_style,
                );
                let mut element_data = ElementData::default();
                element_data.styles.primary = Some(style);
                element_data.set_restyled();
                *doc.nodes[node_id].stylo_element_data.borrow_mut() = Some(element_data);

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

fn create_text_editor(doc: &mut Document, input_element_id: usize, is_multiline: bool) {
    let node = &mut doc.nodes[input_element_id];
    let parley_style = node
        .primary_styles()
        .as_ref()
        .map(|s| stylo_to_parley::style(s))
        .unwrap_or_default();

    let element = &mut node.raw_dom_data.downcast_element_mut().unwrap();
    if !matches!(element.node_specific_data, NodeSpecificData::TextInput(_)) {
        let initial_value = element
            .attr(local_name!("value"))
            .unwrap_or(" ")
            .to_string();

        let mut text_input_data = TextInputData::new(initial_value, 16.0, is_multiline);
        text_input_data
            .editor
            .set_text_size(parley_style.font_size * doc.viewport.scale());
        text_input_data
            .editor
            .set_line_height(parley_style.line_height);
        text_input_data.editor.set_brush(parley_style.brush);
        element.node_specific_data = NodeSpecificData::TextInput(text_input_data);
    }
}

fn create_checkbox_input(doc: &mut Document, input_element_id: usize) {
    let node = &mut doc.nodes[input_element_id];

    let element = &mut node.raw_dom_data.downcast_element_mut().unwrap();
    if !matches!(
        element.node_specific_data,
        NodeSpecificData::CheckboxInput(_)
    ) {
        let checked = element.attr_parsed(local_name!("checked")).unwrap_or(false);

        element.node_specific_data = NodeSpecificData::CheckboxInput(checked);
    }
}

pub(crate) fn build_inline_layout(
    doc: &mut Document,
    inline_context_root_node_id: usize,
) -> (TextLayout, Vec<usize>) {
    // Get the inline context's root node's text styles
    let root_node = &doc.nodes[inline_context_root_node_id];
    let root_node_style = root_node.primary_styles().or_else(|| {
        root_node
            .parent
            .and_then(|parent_id| doc.nodes[parent_id].primary_styles())
    });

    let parley_style = root_node_style
        .as_ref()
        .map(|s| stylo_to_parley::style(s))
        .unwrap_or_default();

    let root_line_height = parley_style.line_height;

    // Create a parley tree builder
    let mut builder =
        doc.layout_ctx
            .tree_builder(&mut doc.font_ctx, doc.viewport.scale(), &parley_style);

    // Set whitespace collapsing mode
    let collapse_mode = root_node_style
        .map(|s| s.get_inherited_text().white_space_collapse)
        .map(stylo_to_parley::white_space_collapse)
        .unwrap_or(WhiteSpaceCollapse::Collapse);
    builder.set_white_space_mode(collapse_mode);

    //Render position-inside list items
    if let Some(ListItemLayout {
        marker,
        position: ListItemLayoutPosition::Inside,
    }) = root_node
        .element_data()
        .and_then(|el| el.list_item_data.as_deref())
    {
        match marker {
            Marker::Char(char) => builder.push_text(&format!("{} ", char)),
            Marker::String(str) => builder.push_text(str),
        }
    };

    for child_id in root_node.children.iter().copied() {
        build_inline_layout_recursive(
            &mut builder,
            &doc.nodes,
            child_id,
            collapse_mode,
            root_line_height,
        );
    }

    let (layout, text) = builder.build();

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
        builder: &mut TreeBuilder<TextBrush>,
        nodes: &Slab<Node>,
        node_id: usize,
        collapse_mode: WhiteSpaceCollapse,
        root_line_height: f32,
    ) {
        let node = &nodes[node_id];

        // Set whitespace collapsing mode
        let collapse_mode = node
            .primary_styles()
            .map(|s| s.get_inherited_text().white_space_collapse)
            .map(stylo_to_parley::white_space_collapse)
            .unwrap_or(collapse_mode);
        builder.set_white_space_mode(collapse_mode);

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

                match (display.outside(), display.inside()) {
                    (DisplayOutside::None, DisplayInside::None) => {}
                    (DisplayOutside::None, DisplayInside::Contents) => {
                        for child_id in node.children.iter().copied() {
                            build_inline_layout_recursive(
                                builder,
                                nodes,
                                child_id,
                                collapse_mode,
                                root_line_height,
                            );
                        }
                    }
                    (DisplayOutside::Inline, DisplayInside::Flow) => {
                        let tag_name = &element_data.name.local;

                        if *tag_name == local_name!("img") || *tag_name == local_name!("input") {
                            builder.push_inline_box(InlineBox {
                                id: node_id as u64,
                                // Overridden by push_inline_box method
                                index: 0,
                                // Width and height are set during layout
                                width: 0.0,
                                height: 0.0,
                            });
                        } else if *tag_name == local_name!("br") {
                            builder.push_style_modification_span(&[]);
                            builder.set_white_space_mode(WhiteSpaceCollapse::Preserve);
                            builder.push_text("\n");
                            builder.pop_style_span();
                            builder.set_white_space_mode(collapse_mode);
                        } else {
                            let mut style = node
                                .primary_styles()
                                .map(|s| stylo_to_parley::style(&s))
                                .unwrap_or_default();

                            // Floor the line-height of the span by the line-height of the inline context
                            // See https://www.w3.org/TR/CSS21/visudet.html#line-height
                            style.line_height = style.line_height.max(root_line_height);

                            builder.push_style_span(style);

                            for child_id in node.children.iter().copied() {
                                build_inline_layout_recursive(
                                    builder,
                                    nodes,
                                    child_id,
                                    collapse_mode,
                                    root_line_height,
                                );
                            }

                            builder.pop_style_span();
                        }
                    }
                    // Inline box
                    (_, _) => {
                        builder.push_inline_box(InlineBox {
                            id: node_id as u64,
                            // Overridden by push_inline_box method
                            index: 0,
                            // Width and height are set during layout
                            width: 0.0,
                            height: 0.0,
                        });
                    }
                };
            }
            NodeData::Text(data) => {
                builder.push_text(&data.content);
            }
            NodeData::Comment => {}
            NodeData::Document => unreachable!(),
        }
    }
}
