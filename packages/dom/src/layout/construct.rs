use html5ever::{local_name, namespace_url, ns, QualName};
use parley::{builder::TreeBuilder, style::TextStyle, style::WhiteSpaceCollapse, InlineBox};
use slab::Slab;
use style::{
    computed_values::white_space::T as StyloWhiteSpaceCollapse,
    data::ElementData,
    properties::ComputedValues,
    shared_lock::StylesheetGuards,
    values::{
        computed::{font::SingleFontFamily, Display},
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

    let container_display = doc.nodes[container_node_id].display_style().unwrap_or(
        match doc.nodes[container_node_id].raw_dom_data.kind() {
            NodeKind::AnonymousBlock => Display::Block,
            _ => Display::Inline,
        },
    );

    match container_display.inside() {
        DisplayInside::None => {},
        DisplayInside::Contents => {

            // Take children array from node to avoid borrow checker issues.
            let children = std::mem::take(&mut doc.nodes[container_node_id].children);

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
            collect_complex_layout_children(doc, container_node_id, layout_children, anonymous_block_id, false, block_item_needs_wrap);
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
            collect_complex_layout_children(doc, container_node_id, layout_children, anonymous_block_id, true, flex_or_grid_item_needs_wrap);
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
    hide_whitespace: bool,
    needs_wrap: impl Fn(NodeKind, DisplayOutside) -> bool,
) {
    // Take children array from node to avoid borrow checker issues.
    let children = std::mem::take(&mut doc.nodes[container_node_id].children);

    for child_id in children.iter().copied() {
        // Get node kind (text, element, comment, etc)
        let child_node_kind = doc.nodes[child_id].raw_dom_data.kind();

        // Get Display style. Default to inline because nodes without styles are probably text nodes
        let child_display = &doc.nodes[child_id]
            .display_style()
            .unwrap_or(Display::inline());
        let display_inside = child_display.inside();
        let display_outside = child_display.outside();

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

pub(crate) fn stylo_to_parley_style(style: &ComputedValues) -> TextStyle<'static, TextBrush> {
    use parley::style::*;

    let font_styles = style.get_font();
    // let text_styles = style.get_text();
    let itext_styles = style.get_inherited_text();

    // Convert font size and line height
    let font_size = font_styles.font_size.used_size.0.px();
    let line_height: f32 = match itext_styles.line_height {
        style::values::generics::text::LineHeight::Normal => font_size * 1.2,
        style::values::generics::text::LineHeight::Number(num) => font_size * num.0,
        style::values::generics::text::LineHeight::Length(value) => value.0.px(),
    };
    // Parley expects line height as a multiple of font size!
    let line_height = line_height / font_size;

    // Convert font family
    let families: Vec<_> = font_styles
        .font_family
        .families
        .list
        .iter()
        .map(|family| match family {
            SingleFontFamily::FamilyName(name) => {
                'ret: {
                    let name = name.name.as_ref();

                    // Legacy web compatibility
                    #[cfg(target_vendor = "apple")]
                    if name == "-apple-system" {
                        break 'ret FontFamily::Generic(GenericFamily::SystemUi);
                    }
                    #[cfg(target_os = "macos")]
                    if name == "BlinkMacSystemFont" {
                        break 'ret FontFamily::Generic(GenericFamily::SystemUi);
                    }

                    // TODO: fix leak!
                    FontFamily::Named(name.to_string().leak())
                }
            }
            SingleFontFamily::Generic(generic) => FontFamily::Generic(match generic {
                style::values::computed::font::GenericFontFamily::None => GenericFamily::SansSerif,
                style::values::computed::font::GenericFontFamily::Serif => GenericFamily::Serif,
                style::values::computed::font::GenericFontFamily::SansSerif => {
                    GenericFamily::SansSerif
                }
                style::values::computed::font::GenericFontFamily::Monospace => {
                    GenericFamily::Monospace
                }
                style::values::computed::font::GenericFontFamily::Cursive => GenericFamily::Cursive,
                style::values::computed::font::GenericFontFamily::Fantasy => GenericFamily::Fantasy,
                style::values::computed::font::GenericFontFamily::SystemUi => {
                    GenericFamily::SystemUi
                }
            }),
        })
        .collect();

    // TODO: fix leak!
    let families: &'static [FontFamily] = Box::leak(families.into_boxed_slice());

    // Convert text colour
    let [r, g, b, a] = itext_styles
        .color
        .to_color_space(style::color::ColorSpace::Srgb)
        .raw_components()
        .map(|f| (f * 255.0) as u8);
    let color = peniko::Color { r, g, b, a };

    TextStyle {
        // font_stack: FontStack::Single(FontFamily::Generic(GenericFamily::SystemUi)),
        font_stack: FontStack::List(families),
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
        letter_spacing: itext_styles.letter_spacing.0.px(),
    }
}

fn white_space_stylo_to_parley(input: StyloWhiteSpaceCollapse) -> WhiteSpaceCollapse {
    match input {
        StyloWhiteSpaceCollapse::Normal => WhiteSpaceCollapse::Collapse,
        StyloWhiteSpaceCollapse::Pre => WhiteSpaceCollapse::Preserve,
        StyloWhiteSpaceCollapse::Nowrap => WhiteSpaceCollapse::Collapse,
        StyloWhiteSpaceCollapse::PreWrap => WhiteSpaceCollapse::Preserve,
        StyloWhiteSpaceCollapse::PreLine => WhiteSpaceCollapse::Preserve,
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
        .map(|s| stylo_to_parley_style(s))
        .unwrap_or_default();

    // Create a parley tree builder
    let mut builder = doc
        .layout_ctx
        .tree_builder(&mut doc.font_ctx, doc.scale, &parley_style);

    // Set whitespace collapsing mode
    let collapse_mode = root_node_style
        .map(|s| s.get_inherited_text().white_space)
        .map(white_space_stylo_to_parley)
        .unwrap_or(WhiteSpaceCollapse::Collapse);
    builder.set_white_space_mode(collapse_mode);

    for child_id in root_node.children.iter().copied() {
        build_inline_layout_recursive(&mut builder, &doc.nodes, child_id, collapse_mode);
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
    ) {
        let node = &nodes[node_id];

        // Set whitespace collapsing mode
        let collapse_mode = node
            .primary_styles()
            .map(|s| s.get_inherited_text().white_space)
            .map(white_space_stylo_to_parley)
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

                match display.inside() {
                    DisplayInside::None => {}
                    DisplayInside::Contents => {
                        for child_id in node.children.iter().copied() {
                            build_inline_layout_recursive(builder, nodes, child_id, collapse_mode);
                        }
                    }
                    DisplayInside::Flow => {
                        if node
                            .raw_dom_data
                            .is_element_with_tag_name(&local_name!("img"))
                        {
                            builder.push_inline_box(InlineBox {
                                id: node_id as u64,
                                // Overridden by push_inline_box method
                                index: 0,
                                // Width and height are set during layout
                                width: 0.0,
                                height: 0.0,
                            });
                        } else {
                            let style = node
                                .primary_styles()
                                .map(|s| stylo_to_parley_style(&s))
                                .unwrap_or_default();
                            builder.push_style_span(style);

                            for child_id in node.children.iter().copied() {
                                build_inline_layout_recursive(
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
