//! Walking the DOM of an inline formatting context into a text backend's builder.

use markup5ever::local_name;
use style::properties::ComputedValues;
use style::values::computed::Display;
use style::values::specified::box_::{DisplayInside, DisplayOutside};

use crate::layout::replaced::is_inline_box_element;
use crate::layout::text_transform::{CaseTransform, TextTransformer};
use crate::node::{ListItemLayout, ListItemLayoutPosition};
use crate::text::{InlineBoxKind, InlineBuilder, SpanKind};
use crate::{Node, NodeData, NodeTree};

/// Pushes the content of the inline formatting context rooted at `root` into `builder`: the
/// marker of a list item whose marker is inside it, then its `::before`, its children and its
/// `::after`, in tree order.
pub(crate) fn push_inline_content<B: InlineBuilder>(
    nodes: &NodeTree,
    root: &Node,
    builder: &mut B,
) {
    // An anonymous block has no style of its own: its text is set in the box around it.
    let root_style = root.primary_styles().or_else(|| {
        root.parent
            .and_then(|parent| nodes[parent].primary_styles())
    });
    let text_transform = case_transform::<B>(root_style.as_deref().map(|style| &**style));
    drop(root_style);

    if let Some(ListItemLayout {
        marker,
        position: ListItemLayoutPosition::Inside,
    }) = root
        .element_data()
        .and_then(|element| element.list_item_data.as_deref())
    {
        builder.push_marker(marker);
    }
    let mut walk = Walk {
        nodes,
        // The marker is a box of its own, so the content's words do not continue from it.
        transformer: TextTransformer::default(),
    };
    walk.transformer.word_break(builder);
    walk.children_and_pseudos(builder, root, &text_transform);
}

/// The text transform text inside an element set in `style` is pushed with, where Blitz
/// transforms text for the backend.
fn case_transform<B: InlineBuilder>(style: Option<&ComputedValues>) -> CaseTransform {
    match style {
        Some(style) if !B::TRANSFORMS_TEXT => CaseTransform::from_style(style),
        _ => CaseTransform::NONE,
    }
}

/// The walk of an inline formatting context's DOM.
struct Walk<'a> {
    nodes: &'a NodeTree,
    /// Transforms text where the backend does not.
    transformer: TextTransformer,
}

impl Walk<'_> {
    /// Pushes `parent`'s `::before`, children and `::after`, whose text is transformed by
    /// `text_transform`.
    fn children_and_pseudos<B: InlineBuilder>(
        &mut self,
        builder: &mut B,
        parent: &Node,
        text_transform: &CaseTransform,
    ) {
        if let Some(before) = parent.before() {
            self.node(builder, &self.nodes[before], text_transform);
        }
        for &child in &parent.children {
            self.node(builder, &self.nodes[child], text_transform);
        }
        if let Some(after) = parent.after() {
            self.node(builder, &self.nodes[after], text_transform);
        }
    }

    /// Pushes `node`, whose text is transformed by its parent's `parent_transform`.
    fn node<B: InlineBuilder>(
        &mut self,
        builder: &mut B,
        node: &Node,
        parent_transform: &CaseTransform,
    ) {
        let element = match &node.data {
            NodeData::Element(element) | NodeData::AnonymousBlock(element) => element,
            NodeData::Text(data) => {
                let text = self
                    .transformer
                    .transform(&data.content, parent_transform, builder);
                builder.push_text(node, text);
                return;
            }
            NodeData::Comment { .. } | NodeData::Document(_) => return,
        };
        if *element.name.local == *"input" && element.attr(local_name!("type")) == Some("hidden") {
            return;
        }
        let Some(style) = node.primary_styles() else {
            return;
        };
        let display = node.display_style().unwrap_or(Display::inline());
        match (display.outside(), display.inside()) {
            (DisplayOutside::None, DisplayInside::None) => {}
            // No box, but its children inherit from it.
            (DisplayOutside::None, DisplayInside::Contents) => {
                let text_transform = case_transform::<B>(Some(&**style));
                builder.push_span(node, &style, SpanKind::Contents);
                self.children_and_pseudos(builder, node, &text_transform);
                builder.pop_span(SpanKind::Contents);
            }
            _ if style.slow_clone_position().is_absolutely_positioned() => {
                builder.push_inline_box(node, &style, InlineBoxKind::Absolute);
            }
            (DisplayOutside::Inline, DisplayInside::Flow) => {
                let tag = &element.name.local;
                if is_inline_box_element(tag) {
                    self.transformer.word_break(builder);
                    builder.push_inline_box(node, &style, InlineBoxKind::Atomic);
                } else if *tag == local_name!("br") {
                    builder.push_line_break(node, &style);
                    self.transformer.word_break(builder);
                } else if *tag == local_name!("wbr") && builder.push_break_opportunity(node) {
                } else if B::SETS_RUBY && *tag == local_name!("rp") {
                    // The fallback parentheses, for a renderer that cannot set ruby.
                } else {
                    let kind = if !B::SETS_RUBY {
                        SpanKind::Span
                    } else if *tag == local_name!("ruby") {
                        SpanKind::Ruby
                    } else if *tag == local_name!("rt") {
                        SpanKind::Annotation {
                            in_container: node.parent.is_some_and(|parent| {
                                self.nodes[parent]
                                    .data
                                    .is_element_with_tag_name(&local_name!("rtc"))
                            }),
                        }
                    } else if *tag == local_name!("rtc") {
                        // An annotation container is the annotation itself where it holds no
                        // `<rt>`; one that does is transparent, each `<rt>` an annotation.
                        let holds_annotation = node.children.iter().any(|&child| {
                            self.nodes[child]
                                .data
                                .is_element_with_tag_name(&local_name!("rt"))
                        });
                        if holds_annotation {
                            SpanKind::Contents
                        } else {
                            SpanKind::Annotation {
                                in_container: false,
                            }
                        }
                    } else {
                        SpanKind::Span
                    };
                    let text_transform = case_transform::<B>(Some(&**style));
                    builder.push_span(node, &style, kind);
                    self.children_and_pseudos(builder, node, &text_transform);
                    builder.pop_span(kind);
                }
            }
            _ if style.slow_clone_float().is_floating() => {
                builder.push_inline_box(node, &style, InlineBoxKind::Float);
            }
            _ => {
                self.transformer.word_break(builder);
                builder.push_inline_box(node, &style, InlineBoxKind::Atomic);
            }
        }
    }
}
