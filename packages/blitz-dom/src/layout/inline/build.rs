use std::{
    ops::Range,
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};

use blitz_traits::node_id::NodeId;
use icu_properties::{CodePointMapData, props::LineBreak as IcuLineBreak};
use markup5ever::local_name;
use parley::{
    FontContext, InlineBox, InlineBoxKind, LayoutContext, LineBreakContext, TreeBuilder,
    WhiteSpaceCollapse,
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
    layout::text_transform::{CaseTransform, TextTransformer},
    node::{ListItemLayout, ListItemLayoutPosition, Marker, TextBrush, TextLayout},
    stylo_to_parley,
};

#[inline]
fn uses_unrestricted_line_break(style: &parley::TextStyle<'_, '_, TextBrush>) -> bool {
    matches!(style.word_break, parley::WordBreak::BreakAll)
        || matches!(style.line_break, parley::LineBreak::Anywhere)
}

#[derive(Default)]
struct LineBreakModeTracker {
    saw_chromium: bool,
    saw_unrestricted: bool,
    unrestricted_ranges: Vec<Range<usize>>,
}

impl LineBreakModeTracker {
    fn push_text(&mut self, builder: &mut TreeBuilder<TextBrush>, text: &str, unrestricted: bool) {
        let start = builder.text().len();
        builder.push_text(text);
        let end = builder.text().len();
        if end > start {
            if unrestricted {
                self.saw_unrestricted = true;
                if let Some(last) = self.unrestricted_ranges.last_mut()
                    && last.end == start
                {
                    last.end = end;
                } else {
                    self.unrestricted_ranges.push(start..end);
                }
            } else {
                self.saw_chromium = true;
            }
        }
    }
}

fn build_pair_chromium_flags(text: &str, unrestricted_ranges: &[Range<usize>]) -> Vec<bool> {
    let lb_map = CodePointMapData::<IcuLineBreak>::new();
    let mut flags = Vec::new();
    let mut prev_char: Option<char> = None;
    let mut prev_is_mandatory = false;
    let mut range_idx = 0;

    for (byte_pos, ch) in text.char_indices() {
        while unrestricted_ranges
            .get(range_idx)
            .is_some_and(|range| range.end <= byte_pos)
        {
            range_idx += 1;
        }
        let is_unrestricted = unrestricted_ranges
            .get(range_idx)
            .is_some_and(|range| range.contains(&byte_pos));

        let is_mandatory = matches!(
            lb_map.get(ch),
            IcuLineBreak::MandatoryBreak
                | IcuLineBreak::CarriageReturn
                | IcuLineBreak::LineFeed
                | IcuLineBreak::NextLine
        );

        if !is_mandatory && !prev_is_mandatory && prev_char.is_some() {
            flags.push(!is_unrestricted);
        }

        prev_char = Some(ch);
        prev_is_mandatory = is_mandatory;
    }

    flags
}

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
    let root_unrestricted = uses_unrestricted_line_break(&parley_style);

    // State only materialized when an inline formatting context mixes normal
    // Chromium ASCII line breaking with `word-break: break-all` or `line-break: anywhere` spans.
    let mixed_override_fn: OnceLock<Box<parley::LineBreakOverrideFn>> = OnceLock::new();

    // Create a parley tree builder
    let mut builder = layout_ctx.tree_builder(font_ctx, scale, true, &parley_style);
    if let Some(style) = root_node_style.as_deref() {
        builder.set_base_direction(stylo_to_parley::base_direction(
            style.clone_direction(),
            style.clone_unicode_bidi(),
        ));
    }

    let text_transform = root_node_style
        .as_deref()
        .map(|s| CaseTransform::from_style(s))
        .unwrap_or(CaseTransform::NONE);
    let mut line_break_tracker = LineBreakModeTracker::default();

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
                let mut marker_style = parley_style.clone();
                marker_style.font_family = BULLET_FONT_FAMILY.into();
                builder.push_style_span(marker_style);
                line_break_tracker.push_text(&mut builder, &format!("{char} "), root_unrestricted);
                builder.pop_style_span();
            }
            Marker::String(str) => {
                line_break_tracker.push_text(&mut builder, str, root_unrestricted);
            }
        }
    };
    // The marker is a separate box, so words in the content don't continue from it.
    let mut text_transformer = TextTransformer::default();
    text_transformer.word_break(&builder);

    if let Some(before_id) = root_node.before() {
        build_inline_layout_recursive(
            &mut builder,
            &mut text_transformer,
            &mut line_break_tracker,
            nodes,
            before_id,
            &text_transform,
            root_unrestricted,
        );
    }
    for child_id in root_node.children.iter().copied() {
        build_inline_layout_recursive(
            &mut builder,
            &mut text_transformer,
            &mut line_break_tracker,
            nodes,
            child_id,
            &text_transform,
            root_unrestricted,
        );
    }
    if let Some(after_id) = root_node.after() {
        build_inline_layout_recursive(
            &mut builder,
            &mut text_transformer,
            &mut line_break_tracker,
            nodes,
            after_id,
            &text_transform,
            root_unrestricted,
        );
    }

    // Apply Blink/WebKit-compatible ASCII line-breaking overrides unless unrestricted
    // character/letter breaking (`word-break: break-all` or `line-break: anywhere`) is active,
    // since Parley's `line_break_override` takes precedence over `BreakAll` and `Anywhere`.
    if !line_break_tracker.saw_unrestricted {
        builder.set_line_break_override(Some(parley::CHROMIUM_LINE_BREAK_OVERRIDE));
    } else if line_break_tracker.saw_chromium {
        let flags =
            build_pair_chromium_flags(builder.text(), &line_break_tracker.unrestricted_ranges);
        let cursor = AtomicUsize::new(0);
        let override_fn = mixed_override_fn.get_or_init(|| {
            Box::new(move |cx: LineBreakContext| -> Option<bool> {
                let idx = cursor.fetch_add(1, Ordering::Relaxed);
                if flags.get(idx).copied().unwrap_or(true) {
                    (parley::CHROMIUM_LINE_BREAK_OVERRIDE)(cx)
                } else {
                    None
                }
            })
        });
        builder.set_line_break_override(Some(&**override_fn));
    }

    text_layout.text = builder.build_into(&mut text_layout.layout);
    return;

    fn build_inline_layout_recursive(
        builder: &mut TreeBuilder<TextBrush>,
        text_transformer: &mut TextTransformer,
        line_break_tracker: &mut LineBreakModeTracker,
        nodes: &crate::NodeTree,
        node_id: NodeId,
        parent_text_transform: &CaseTransform,
        parent_unrestricted: bool,
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
                    .map(|s| s.clone_position())
                    .unwrap_or(PositionProperty::Static);
                let float = style.map(|s| s.clone_float()).unwrap_or(Float::None);
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
                                        s.clone_white_space_collapse(),
                                    ),
                                ),
                                parley::StyleProperty::TextWrapMode(
                                    stylo_to_parley::text_wrap_mode(s.clone_text_wrap_mode()),
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
                                line_break_tracker,
                                nodes,
                                child_id,
                                &text_transform,
                                parent_unrestricted,
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
                            let span_unrestricted = uses_unrestricted_line_break(&style);

                            builder.push_style_span(style);

                            if let Some(before_id) = node.before() {
                                build_inline_layout_recursive(
                                    builder,
                                    text_transformer,
                                    line_break_tracker,
                                    nodes,
                                    before_id,
                                    &text_transform,
                                    span_unrestricted,
                                );
                            }

                            for child_id in node.children.iter().copied() {
                                build_inline_layout_recursive(
                                    builder,
                                    text_transformer,
                                    line_break_tracker,
                                    nodes,
                                    child_id,
                                    &text_transform,
                                    span_unrestricted,
                                );
                            }
                            if let Some(after_id) = node.after() {
                                build_inline_layout_recursive(
                                    builder,
                                    text_transformer,
                                    line_break_tracker,
                                    nodes,
                                    after_id,
                                    &text_transform,
                                    span_unrestricted,
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
                line_break_tracker.push_text(builder, text, parent_unrestricted);
            }
            NodeData::Comment { .. } => {
                // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            }
            NodeData::Document(_) => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use blitz_traits::shell::{ColorScheme, Viewport};

    use crate::{Attribute, BaseDocument, DocumentConfig, NodeId, node::TextLayout, qual_name};

    fn style_attr(value: &str) -> Attribute {
        Attribute {
            name: qual_name!("style"),
            value: value.to_string(),
        }
    }

    fn make_inline_doc(container_style: &str, text_content: &str) -> (BaseDocument, NodeId) {
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            ..Default::default()
        });
        let root_id = doc.root_node().id;

        let mut mutator = doc.mutate();
        let html = mutator.create_element(qual_name!("html"), vec![style_attr("display:block")]);
        let body = mutator.create_element(
            qual_name!("body"),
            vec![style_attr("display:block;margin:0")],
        );
        let container = mutator.create_element(
            qual_name!("div"),
            vec![style_attr(&format!("display:block;{container_style}"))],
        );
        let text = mutator.create_text_node(text_content);
        mutator.append_children(container, &[text]);
        mutator.append_children(body, &[container]);
        mutator.append_children(html, &[body]);
        mutator.append_children(root_id, &[html]);
        drop(mutator);

        doc.resolve(0.0);
        (doc, container)
    }

    fn inline_layout(doc: &BaseDocument, container: NodeId) -> &TextLayout {
        doc.nodes[container]
            .element_data()
            .unwrap()
            .inline_layout_data
            .as_deref()
            .unwrap()
    }

    #[test]
    fn default_overflow_wrap_does_not_break_ascii_slash_or_pipe_tokens() {
        for token in [
            "https://example.com/path/segment",
            "10:playground|16:tooltip",
        ] {
            for width_px in [60, 184] {
                let css = format!("width:{width_px}px;font-family:monospace;font-size:16px");
                let (doc, container) = make_inline_doc(&css, token);
                let text_layout = inline_layout(&doc, container);
                let widths = text_layout.layout.calculate_content_widths();
                if widths.max == 0.0 {
                    eprintln!("skipping: no usable font (text measures 0x0)");
                    return;
                }

                assert!(
                    widths.max > width_px as f32,
                    "expected {token:?} max-content width ({}) to exceed {width_px}px",
                    widths.max
                );
                assert!(
                    (widths.min - widths.max).abs() < 0.01,
                    "expected min-content width ({}) to match max-content width ({}) for {token:?}",
                    widths.min,
                    widths.max
                );
                assert_eq!(
                    text_layout.layout.len(),
                    1,
                    "expected {token:?} not to soft-wrap under default overflow-wrap: normal at {width_px}px"
                );
            }
        }
    }

    #[test]
    fn overflow_wrap_anywhere_and_word_break_break_all_wrap_ascii_slash_and_pipe_tokens() {
        for token in [
            "https://example.com/path/segment",
            "10:playground|16:tooltip",
        ] {
            for rule in ["overflow-wrap:anywhere", "word-break:break-all"] {
                let css = format!("width:184px;font-family:monospace;font-size:16px;{rule}");
                let (doc, container) = make_inline_doc(&css, token);
                let text_layout = inline_layout(&doc, container);
                let widths = text_layout.layout.calculate_content_widths();
                if widths.max == 0.0 {
                    eprintln!("skipping: no usable font (text measures 0x0)");
                    return;
                }

                assert!(
                    widths.max > 184.0,
                    "expected {token:?} max-content width ({}) to exceed 184px",
                    widths.max
                );
                assert!(
                    widths.min < 184.0,
                    "expected {token:?} min-content width ({}) under {rule} to be < 184px",
                    widths.min
                );
                assert!(
                    text_layout.layout.len() >= 2,
                    "expected {token:?} under {rule} to wrap across multiple lines inside 184px"
                );
                for line in text_layout.layout.lines() {
                    let visible_advance = line.metrics().advance - line.metrics().hanging_advance;
                    assert!(
                        visible_advance <= 184.5,
                        "expected wrapped line advance ({visible_advance}) <= 184px under {rule}"
                    );
                }
            }
        }
    }

    #[test]
    fn space_delimited_words_soft_wrap_normally() {
        let text = "alpha beta gamma delta epsilon";
        let css = "width:184px;font-family:monospace;font-size:16px";
        let (doc, container) = make_inline_doc(css, text);
        let text_layout = inline_layout(&doc, container);
        let widths = text_layout.layout.calculate_content_widths();
        if widths.max == 0.0 {
            eprintln!("skipping: no usable font (text measures 0x0)");
            return;
        }

        assert!(
            widths.max > 184.0,
            "expected {text:?} max-content width ({}) to exceed 184px",
            widths.max
        );
        assert!(
            widths.min < 184.0,
            "expected {text:?} min-content width ({}) to be < 184px",
            widths.min
        );
        assert!(
            text_layout.layout.len() >= 2,
            "expected space-delimited words to soft-wrap inside 184px"
        );
        for line in text_layout.layout.lines() {
            let visible_advance = line.metrics().advance - line.metrics().hanging_advance;
            assert!(
                visible_advance <= 184.5,
                "expected wrapped line advance ({visible_advance}) <= 184px"
            );
        }
    }

    #[test]
    fn mixed_normal_and_unrestricted_spans_preserve_per_span_breaking() {
        for span_rule in ["word-break:break-all", "line-break:anywhere"] {
            let mut doc = BaseDocument::new(DocumentConfig {
                viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
                ..Default::default()
            });
            let root_id = doc.root_node().id;

            let mut mutator = doc.mutate();
            let html =
                mutator.create_element(qual_name!("html"), vec![style_attr("display:block")]);
            let body = mutator.create_element(
                qual_name!("body"),
                vec![style_attr("display:block;margin:0")],
            );
            let container = mutator.create_element(
                qual_name!("div"),
                vec![style_attr(
                    "display:block;width:80px;font-family:monospace;font-size:16px",
                )],
            );
            let normal_slash = mutator.create_text_node("ab/cd ");
            let span = mutator.create_element(qual_name!("span"), vec![style_attr(span_rule)]);
            let span_text = mutator.create_text_node("10:playground|16:tooltip");
            mutator.append_children(span, &[span_text]);
            mutator.append_children(container, &[normal_slash, span]);
            mutator.append_children(body, &[container]);
            mutator.append_children(html, &[body]);
            mutator.append_children(root_id, &[html]);
            drop(mutator);

            doc.resolve(0.0);

            let text_layout = inline_layout(&doc, container);
            let widths = text_layout.layout.calculate_content_widths();
            if widths.max == 0.0 {
                eprintln!("skipping: no usable font (text measures 0x0)");
                return;
            }

            let lines: Vec<&str> = text_layout
                .layout
                .lines()
                .map(|line| &text_layout.text[line.text_range()])
                .collect();
            assert!(
                lines[0].starts_with("ab/cd"),
                "expected normal prefix \"ab/cd\" to stay unbroken on first line, got {lines:?}"
            );
            assert!(
                lines.len() >= 3,
                "expected {span_rule} span to wrap across multiple lines in 80px, got {lines:?}"
            );
        }
    }
}
