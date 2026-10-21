use blitz_traits::node_id::NodeId;
use markup5ever::local_name;
use parley::{
    FontContext, InlineBox, InlineBoxKind, LayoutContext, TreeBuilder, WhiteSpaceCollapse,
};
use style::{
    computed_values::position::T as PositionProperty,
    values::{
        computed::{Display, Float},
        specified::box_::{DisplayInside, DisplayOutside},
    },
};

use crate::{
    NodeData,
    layout::list::BULLET_FONT_FAMILY,
    layout::replaced::is_inline_box_element,
    layout::text_transform::{CaseTransform, TextTransformer},
    node::{ListItemLayout, ListItemLayoutPosition, Marker, TextBrush, TextLayout},
    stylo_to_parley,
};

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

    // Create a parley tree builder
    let mut builder = layout_ctx.tree_builder(font_ctx, scale, true, &parley_style);
    if let Some(style) = root_node_style.as_deref() {
        builder.set_base_direction(stylo_to_parley::base_direction(
            style.slow_clone_direction(),
            style.slow_clone_unicode_bidi(),
        ));
    }

    let text_transform = root_node_style
        .as_deref()
        .map(|s| CaseTransform::from_style(s))
        .unwrap_or(CaseTransform::NONE);

    // Render position-inside list items
    if let Some(ListItemLayout {
        marker,
        position: ListItemLayoutPosition::Inside,
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
                let mut marker_style = parley_style.clone();
                marker_style.font_family = BULLET_FONT_FAMILY.into();
                builder.push_style_span(marker_style);
                builder.push_text(&format!("{char} "));
                builder.pop_style_span();
            }
            Marker::String(str) => builder.push_text(str),
        }
    };
    // The marker is a separate box, so words in the content don't continue from it.
    let mut text_transformer = TextTransformer::default();
    text_transformer.word_break(&builder);

    if let Some(before_id) = root_node.before() {
        build_inline_layout_recursive(
            &mut builder,
            &mut text_transformer,
            nodes,
            before_id,
            &text_transform,
        );
    }
    for child_id in root_node.children.iter().copied() {
        build_inline_layout_recursive(
            &mut builder,
            &mut text_transformer,
            nodes,
            child_id,
            &text_transform,
        );
    }
    if let Some(after_id) = root_node.after() {
        build_inline_layout_recursive(
            &mut builder,
            &mut text_transformer,
            nodes,
            after_id,
            &text_transform,
        );
    }

    text_layout.text = builder.build_into(&mut text_layout.layout);
    return;

    fn build_inline_layout_recursive(
        builder: &mut TreeBuilder<TextBrush>,
        text_transformer: &mut TextTransformer,
        nodes: &crate::NodeTree,
        node_id: NodeId,
        parent_text_transform: &CaseTransform,
    ) {
        let node = &nodes[node_id];

        let style = node.primary_styles();
        let style = style.as_ref();

        let text_transform = style
            .map(|s| CaseTransform::from_style(s))
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
                    .map(|s| s.slow_clone_position())
                    .unwrap_or(PositionProperty::Static);
                let float = style.map(|s| s.slow_clone_float()).unwrap_or(Float::None);
                let box_kind = if position.is_absolutely_positioned() {
                    InlineBoxKind::OutOfFlow
                } else if float.is_floating() {
                    InlineBoxKind::CustomOutOfFlow
                } else {
                    InlineBoxKind::InFlow
                };

                match (display.outside(), display.inside()) {
                    (DisplayOutside::None, DisplayInside::None) => {
                        // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                    }
                    (DisplayOutside::None, DisplayInside::Contents) => {
                        let whitespace = style.map(|s| {
                            [
                                parley::StyleProperty::WhiteSpaceCollapse(
                                    stylo_to_parley::white_space_collapse(
                                        s.slow_clone_white_space_collapse(),
                                    ),
                                ),
                                parley::StyleProperty::TextWrapMode(
                                    stylo_to_parley::text_wrap_mode(s.slow_clone_text_wrap_mode()),
                                ),
                            ]
                        });
                        builder.push_style_modification_span(
                            whitespace
                                .as_ref()
                                .map_or(&[][..], |styles| styles.as_slice()),
                        );
                        for child_id in node.children.iter().copied() {
                            // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                            build_inline_layout_recursive(
                                builder,
                                text_transformer,
                                nodes,
                                child_id,
                                &text_transform,
                            );
                        }
                        builder.pop_style_span();
                    }
                    (DisplayOutside::Inline, DisplayInside::Flow) => {
                        let tag_name = &element_data.name.local;

                        if is_inline_box_element(tag_name) {
                            if box_kind == InlineBoxKind::InFlow {
                                text_transformer.word_break(builder);
                            }
                            builder.push_inline_box(InlineBox {
                                id: node_id.as_u64(),
                                kind: box_kind,
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
                            });
                        } else if *tag_name == local_name!("br") {
                            // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                            // TODO: update span id for br spans
                            builder.push_style_modification_span(&[
                                parley::StyleProperty::WhiteSpaceCollapse(
                                    WhiteSpaceCollapse::Preserve,
                                ),
                            ]);
                            builder.push_text("\n");
                            builder.pop_style_span();
                            text_transformer.word_break(builder);
                        } else {
                            // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                            let style = node
                                .primary_styles()
                                .map(|s| stylo_to_parley::style(node.id, &s))
                                .unwrap_or_else(|| parley::TextStyle {
                                    white_space_collapse: WhiteSpaceCollapse::Collapse,
                                    ..Default::default()
                                });

                            builder.push_style_span(style);

                            if let Some(before_id) = node.before() {
                                build_inline_layout_recursive(
                                    builder,
                                    text_transformer,
                                    nodes,
                                    before_id,
                                    &text_transform,
                                );
                            }

                            for child_id in node.children.iter().copied() {
                                build_inline_layout_recursive(
                                    builder,
                                    text_transformer,
                                    nodes,
                                    child_id,
                                    &text_transform,
                                );
                            }
                            if let Some(after_id) = node.after() {
                                build_inline_layout_recursive(
                                    builder,
                                    text_transformer,
                                    nodes,
                                    after_id,
                                    &text_transform,
                                );
                            }

                            builder.pop_style_span();
                        }
                    }
                    // Inline box
                    (_, _) => {
                        if box_kind == InlineBoxKind::InFlow {
                            text_transformer.word_break(builder);
                        }
                        builder.push_inline_box(InlineBox {
                            id: node_id.as_u64(),
                            kind: box_kind,
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
                        });
                    }
                };
            }
            NodeData::Text(data) => {
                // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                // dbg!(&data.content);

                let text =
                    text_transformer.transform(&data.content, parent_text_transform, builder);
                builder.push_text(text);
            }
            NodeData::Comment { .. } => {
                // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            }
            NodeData::Document(_) => unreachable!(),
        }
    }
}
