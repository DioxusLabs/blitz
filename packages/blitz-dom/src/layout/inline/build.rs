//! Walking the DOM of an inline formatting context into a text backend's builder.

use markup5ever::local_name;
use style::properties::ComputedValues;
use style::values::computed::Display;
use style::values::specified::box_::{DisplayInside, DisplayOutside};

use crate::layout::replaced::is_inline_box_element;
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

    if let Some(ListItemLayout {
        marker,
        position: ListItemLayoutPosition::Inside,
    }) = root
        .element_data()
        .and_then(|element| element.list_item_data.as_deref())
    {
        builder.push_marker(marker);
    }
    let walk = Walk { nodes };
    walk.children_and_pseudos(builder, root, root_style.as_deref().map(|style| &**style));
}

/// The walk of an inline formatting context's DOM.
struct Walk<'a> {
    nodes: &'a NodeTree,
}

impl Walk<'_> {
    /// Pushes `parent`'s `::before`, children and `::after`, whose text is set in `style`.
    fn children_and_pseudos<B: InlineBuilder>(
        &self,
        builder: &mut B,
        parent: &Node,
        style: Option<&ComputedValues>,
    ) {
        if let Some(before) = parent.before() {
            self.node(builder, &self.nodes[before], style);
        }
        for &child in &parent.children {
            self.node(builder, &self.nodes[child], style);
        }
        if let Some(after) = parent.after() {
            self.node(builder, &self.nodes[after], style);
        }
    }

    /// Pushes `node`, whose text is set in its parent's `parent_style`.
    fn node<B: InlineBuilder>(
        &self,
        builder: &mut B,
        node: &Node,
        parent_style: Option<&ComputedValues>,
    ) {
        let element = match &node.data {
            NodeData::Element(element) | NodeData::AnonymousBlock(element) => element,
            NodeData::Text(data) => {
                builder.push_text(node, &data.content, parent_style);
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
                builder.push_span(node, &style, SpanKind::Contents);
                self.children_and_pseudos(builder, node, Some(&**style));
                builder.pop_span(SpanKind::Contents);
            }
            _ if style.clone_position().is_absolutely_positioned() => {
                builder.push_inline_box(node, &style, InlineBoxKind::Absolute);
            }
            (DisplayOutside::Inline, DisplayInside::Flow) => {
                let tag = &element.name.local;
                if is_inline_box_element(tag) {
                    builder.push_inline_box(node, &style, InlineBoxKind::Atomic);
                } else if *tag == local_name!("br") {
                    builder.push_line_break(node, &style);
                } else if *tag == local_name!("wbr") && builder.push_break_opportunity(node) {
                } else {
                    let kind = if *tag == local_name!("ruby") {
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
                            SpanKind::AnnotationContainer
                        } else {
                            SpanKind::Annotation {
                                in_container: false,
                            }
                        }
                    } else {
                        SpanKind::Span
                    };
                    builder.push_span(node, &style, kind);
                    self.children_and_pseudos(builder, node, Some(&**style));
                    builder.pop_span(kind);
                }
            }
            _ if style.clone_float().is_floating() => {
                builder.push_inline_box(node, &style, InlineBoxKind::Float);
            }
            _ => {
                builder.push_inline_box(node, &style, InlineBoxKind::Atomic);
            }
        }
    }
}
