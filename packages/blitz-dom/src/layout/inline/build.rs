use blitz_traits::node_id::NodeId;
use markup5ever::local_name;
use parley::{
    FloatedBox, FontContext, InlineBox, LayoutContext, StyleRunBuilder, WhiteSpaceCollapse,
};
use style::{
    computed_values::position::T as PositionProperty,
    values::{
        computed::{Display, Float},
        specified::box_::{DisplayInside, DisplayOutside},
    },
};
use stylo_taffy::CopyStyleGetters;

use crate::{
    NodeData,
    layout::list::BULLET_FONT_FAMILY,
    layout::replaced::is_inline_box_element,
    node::{ListItemLayout, ListItemLayoutPosition, Marker, OutOfFlowBox, TextBrush, TextLayout},
    stylo_to_parley,
};
use parley_css_text::{CaseTransform, TextTransformer, WhiteSpace, WhiteSpaceCollapser};

pub(crate) fn build_inline_layout_into(
    nodes: &crate::NodeTree,
    layout_ctx: &mut LayoutContext<TextBrush>,
    font_ctx: &mut FontContext,
    text_layout: &mut TextLayout,
    scale: f32,
    inline_context_root_node_id: NodeId,
) {
    // Get the inline context's root node's text styles
    let root_node = &nodes[inline_context_root_node_id];
    let root_node_style = root_node.primary_styles().or_else(|| {
        root_node
            .parent
            .and_then(|parent_id| nodes[parent_id].primary_styles())
    });

    let parley_style = root_node_style
        .as_ref()
        .map(|s| stylo_to_parley::style(inline_context_root_node_id, s))
        .unwrap_or_else(|| parley::TextStyle {
            white_space_collapse: WhiteSpaceCollapse::Collapse,
            ..Default::default()
        });

    // Create a parley style run builder
    let mut builder = layout_ctx.style_run_builder(font_ctx, scale, true);
    let root_style_index = builder.push_style(parley_style, None);
    let root_span = StyleSpan {
        index: root_style_index,
        parent: None,
    };
    if let Some(style) = root_node_style.as_deref() {
        builder.set_base_direction(stylo_to_parley::base_direction(
            style.clone_direction(),
            style.clone_unicode_bidi(),
        ));
    }

    let mut collapser = WhiteSpaceCollapser::with_text(std::mem::take(&mut text_layout.text));
    let root_white_space = white_space(root_node_style.as_deref().map(|style| &**style));

    let text_transform = root_node_style
        .as_deref()
        .map(|s| stylo_to_parley::case_transform(s))
        .unwrap_or(CaseTransform::NONE);

    // Render position-inside list items
    if let Some(ListItemLayout {
        marker,
        position: ListItemLayoutPosition::Inside,
        ..
    }) = root_node
        .element_data()
        .and_then(|el| el.list_item_data.as_deref())
    {
        match marker {
            // Bullet glyphs live in the bundled bullet font. The position-outside
            // path already asks for it; without the same span here a marker like
            // disclosure-closed (U+25B8) falls back to the element's own font and
            // renders as a missing glyph.
            Marker::Char(char) => {
                let marker_style_index = builder.push_modified_style(
                    root_style_index,
                    None,
                    [parley::StyleProperty::FontFamily(BULLET_FONT_FAMILY.into())],
                );
                collapser.push_generated(
                    &mut builder,
                    marker_style_index,
                    None,
                    inline_context_root_node_id,
                    &format!("{char} "),
                    root_white_space,
                );
            }
            Marker::String(str) => collapser.push_generated(
                &mut builder,
                root_style_index,
                None,
                inline_context_root_node_id,
                str,
                root_white_space,
            ),
        }
    };
    text_layout.out_of_flow_boxes.clear();
    let oof_boxes = &mut text_layout.out_of_flow_boxes;

    // The marker is a separate box, so words in the content don't continue from it.
    let mut text_transformer = TextTransformer::default();
    text_transformer.word_break(collapser.text().len());

    if let Some(before_id) = root_node.before() {
        build_inline_layout_recursive(
            &mut builder,
            &mut collapser,
            &mut text_transformer,
            oof_boxes,
            nodes,
            before_id,
            root_span,
            &text_transform,
        );
    }
    for child_id in root_node.children.iter().copied() {
        build_inline_layout_recursive(
            &mut builder,
            &mut collapser,
            &mut text_transformer,
            oof_boxes,
            nodes,
            child_id,
            root_span,
            &text_transform,
        );
    }
    if let Some(after_id) = root_node.after() {
        build_inline_layout_recursive(
            &mut builder,
            &mut collapser,
            &mut text_transformer,
            oof_boxes,
            nodes,
            after_id,
            root_span,
            &text_transform,
        );
    }

    let (text, offset_map) = collapser.finish();
    builder.build_into(&mut text_layout.layout, &text);
    text_layout.text = text;
    text_layout.offset_map = offset_map;
    return;

    #[derive(Clone, Copy)]
    struct StyleSpan {
        index: u16,
        parent: Option<u16>,
    }

    #[allow(clippy::too_many_arguments)]
    fn build_inline_layout_recursive(
        builder: &mut StyleRunBuilder<TextBrush>,
        collapser: &mut WhiteSpaceCollapser<NodeId>,
        text_transformer: &mut TextTransformer,
        oof_boxes: &mut Vec<OutOfFlowBox>,
        nodes: &crate::NodeTree,
        node_id: NodeId,
        parent_span: StyleSpan,
        parent_text_transform: &CaseTransform,
    ) {
        let node = &nodes[node_id];

        let style = node.primary_styles();
        let style = style.as_ref();

        let text_transform = style
            .map(|s| stylo_to_parley::case_transform(s))
            .unwrap_or(CaseTransform::NONE);

        match &node.data {
            NodeData::Element(element_data) | NodeData::AnonymousBlock(element_data) => {
                // if the input type is hidden, hide it
                if *element_data.name.local == *"input" {
                    if let Some("hidden") = element_data.attr(local_name!("type")) {
                        return;
                    }
                }

                let display = node.display_style().unwrap_or(Display::inline());
                let position = style
                    .map(|s| s.clone_position())
                    .unwrap_or(PositionProperty::Static);
                let float = style.map(|s| s.clone_float()).unwrap_or(Float::None);
                let is_absolute = position.is_absolutely_positioned();
                let is_floated = !is_absolute && cfg!(feature = "floats") && float.is_floating();
                let mut push_box =
                    |builder: &mut StyleRunBuilder<TextBrush>,
                     collapser: &mut WhiteSpaceCollapser<NodeId>,
                     text_transformer: &mut TextTransformer| {
                        if is_absolute || is_floated {
                            // Parley has no notion of absolutely positioned boxes: their static
                            // position is derived from their text index after layout. Like Parley's
                            // floated boxes, they precede any pending collapsible whitespace.
                            let index = collapser.push_out_of_flow_box(node_id);
                            oof_boxes.push(OutOfFlowBox {
                                node_id,
                                index,
                                is_floated,
                            });
                            if is_floated {
                                builder.push_floated_box(FloatedBox {
                                    id: node_id.as_u64(),
                                    index,
                                });
                            }
                        } else {
                            collapser.push_inline_box(
                                builder,
                                parent_span.index,
                                node_id,
                                InlineBox {
                                    id: node_id.as_u64(),
                                    // Overridden by push_inline_box method
                                    index: 0,
                                    // Width and height are set during layout
                                    width: 0.0,
                                    height: 0.0,
                                    baseline: None,
                                    vertical_align: node
                                        .primary_styles()
                                        .map(|s| stylo_to_parley::vertical_align(&s))
                                        .unwrap_or_default(),
                                },
                            );
                            text_transformer.word_break(collapser.text().len());
                        }
                    };

                match (display.outside(), display.inside()) {
                    (DisplayOutside::None, DisplayInside::None) => {
                        // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                    }
                    (DisplayOutside::None, DisplayInside::Contents) => {
                        let child_span = style.map_or(parent_span, |s| StyleSpan {
                            index: builder.push_modified_style(
                                parent_span.index,
                                parent_span.parent,
                                [
                                    parley::StyleProperty::WhiteSpaceCollapse(
                                        stylo_to_parley::white_space_collapse(
                                            s.clone_white_space_collapse(),
                                        ),
                                    ),
                                    parley::StyleProperty::TextWrapMode(
                                        stylo_to_parley::text_wrap_mode(s.clone_text_wrap_mode()),
                                    ),
                                    parley::StyleProperty::CollapsedSpace(
                                        stylo_to_parley::collapsed_space(s.clone_text_transform()),
                                    ),
                                ],
                            ),
                            parent: parent_span.parent,
                        });
                        if let Some(before_id) = node.before() {
                            build_inline_layout_recursive(
                                builder,
                                collapser,
                                text_transformer,
                                oof_boxes,
                                nodes,
                                before_id,
                                child_span,
                                &text_transform,
                            );
                        }
                        for child_id in node.children.iter().copied() {
                            // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                            build_inline_layout_recursive(
                                builder,
                                collapser,
                                text_transformer,
                                oof_boxes,
                                nodes,
                                child_id,
                                child_span,
                                &text_transform,
                            );
                        }
                        if let Some(after_id) = node.after() {
                            build_inline_layout_recursive(
                                builder,
                                collapser,
                                text_transformer,
                                oof_boxes,
                                nodes,
                                after_id,
                                child_span,
                                &text_transform,
                            );
                        }
                    }
                    (DisplayOutside::Inline, DisplayInside::Flow) => {
                        let tag_name = &element_data.name.local;

                        if is_inline_box_element(tag_name) {
                            push_box(builder, collapser, text_transformer);
                        } else if *tag_name == local_name!("br") {
                            // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                            // TODO: update span id for br spans
                            collapser.push_line_break(builder, parent_span.index, node_id);
                            text_transformer.word_break(collapser.text().len());
                        } else if node.before().is_some()
                            || !node.children.is_empty()
                            || node.after().is_some()
                        {
                            // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                            let style = node
                                .primary_styles()
                                .map(|s| stylo_to_parley::style(node.id, &s))
                                .unwrap_or_else(|| parley::TextStyle {
                                    white_space_collapse: WhiteSpaceCollapse::Collapse,
                                    ..Default::default()
                                });

                            let style_index = builder.push_style(style, Some(parent_span.index));
                            let child_span = StyleSpan {
                                index: style_index,
                                parent: Some(parent_span.index),
                            };

                            if let Some(before_id) = node.before() {
                                build_inline_layout_recursive(
                                    builder,
                                    collapser,
                                    text_transformer,
                                    oof_boxes,
                                    nodes,
                                    before_id,
                                    child_span,
                                    &text_transform,
                                );
                            }

                            for child_id in node.children.iter().copied() {
                                build_inline_layout_recursive(
                                    builder,
                                    collapser,
                                    text_transformer,
                                    oof_boxes,
                                    nodes,
                                    child_id,
                                    child_span,
                                    &text_transform,
                                );
                            }
                            if let Some(after_id) = node.after() {
                                build_inline_layout_recursive(
                                    builder,
                                    collapser,
                                    text_transformer,
                                    oof_boxes,
                                    nodes,
                                    after_id,
                                    child_span,
                                    &text_transform,
                                );
                            }
                        }
                    }
                    // Inline box
                    (_, _) => {
                        push_box(builder, collapser, text_transformer);
                    }
                };
            }
            NodeData::Text(data) => {
                // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                // dbg!(&data.content);

                let (text, changes) = text_transformer.transform(
                    &data.content,
                    parent_text_transform,
                    collapser.text(),
                    collapser.has_pending_whitespace(),
                );
                let white_space = white_space(text_style(nodes, node_id).as_deref());
                collapser.push_text(
                    builder,
                    parent_span.index,
                    parent_span.parent,
                    node_id,
                    &data.content,
                    text,
                    changes,
                    white_space,
                );
            }
            NodeData::Comment { .. } => {
                // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            }
            NodeData::Document(_) => unreachable!(),
        }
    }
}

/// The styles that apply to the text node `node_id`: those of its nearest styled ancestor.
fn text_style(
    nodes: &crate::NodeTree,
    node_id: NodeId,
) -> Option<style::servo_arc::Arc<style::properties::ComputedValues>> {
    let mut parent = nodes[node_id].parent;
    while let Some(id) = parent {
        if let Some(style) = nodes[id].primary_styles() {
            return Some(style.clone());
        }
        parent = nodes[id].parent;
    }
    None
}

fn white_space(style: Option<&style::properties::ComputedValues>) -> WhiteSpace {
    let Some(style) = style else {
        return WhiteSpace {
            collapse: WhiteSpaceCollapse::Collapse,
            collapsed_space: " ",
            wrap: true,
        };
    };
    WhiteSpace {
        collapse: stylo_to_parley::white_space_collapse(style.clone_white_space_collapse()),
        collapsed_space: match stylo_to_parley::collapsed_space(style.clone_text_transform()) {
            parley::CollapsedSpace::IdeographicSpace => "\u{3000}",
            parley::CollapsedSpace::Space => " ",
        },
        wrap: style.clone_text_wrap_mode() == stylo_to_parley::stylo::TextWrapMode::Wrap,
    }
}
