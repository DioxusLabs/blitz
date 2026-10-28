//! Walking the DOM of an inline formatting context into a text backend's builder.

use markup5ever::local_name;
use style::properties::ComputedValues;
use style::values::computed::Display;
use style::values::specified::box_::{DisplayInside, DisplayOutside};

use core::ops::Range;

use crate::layout::replaced::is_inline_box_element;
use crate::layout::text_transform::{CaseTransform, TextTransformer};
use crate::node::{ListItemLayout, ListItemLayoutPosition};
use crate::text::{InlineBoxKind, InlineBuilder, SpanKind};
use crate::{Node, NodeData, NodeId, NodeTree};

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
                    if kind == SpanKind::Ruby {
                        self.ruby_children(builder, node, &text_transform);
                    } else {
                        self.children_and_pseudos(builder, node, &text_transform);
                    }
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

/// What a child of a ruby container or of an annotation container is to CSS Ruby's box fixup
/// (CSS Ruby 1, section 2.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RubyChild {
    /// A ruby base, `<rb>`.
    Base,
    /// A ruby annotation, `<rt>`.
    Annotation,
    /// An annotation container, `<rtc>`.
    Container,
    /// Collapsible white space, which pairs apart from the boxes beside it.
    Space,
    /// Other inline content, which an anonymous base or annotation wraps.
    Inline,
    /// Content with no box, such as `<rp>` or `display: none`.
    Nothing,
}

/// A ruby base or annotation, and the white space before it in its level.
#[derive(Clone, Debug, Default)]
struct RubyUnit {
    /// The children it is made of, as a range of the walk's list: one element, or a run of inline
    /// content that CSS wraps in an anonymous box.
    nodes: Range<usize>,
    /// Whether it is such a run.
    anonymous: bool,
    /// The white space between it and the unit before it in its level.
    space: Range<usize>,
}

/// An annotation level of a ruby segment: `<rt>`s in a row, or an `<rtc>`.
#[derive(Debug)]
struct RubyLevel {
    /// The `<rtc>`, where the level is one.
    container: Option<NodeId>,
    /// Its annotations in order. An `<rtc>` holding no `<rt>` has none: it is one anonymous
    /// annotation, which spans every base of its segment.
    units: Vec<RubyUnit>,
}

/// A ruby segment: a run of bases and the annotation levels after it.
#[derive(Debug, Default)]
struct RubySegment {
    /// The white space before its first base.
    lead: Range<usize>,
    bases: Vec<RubyUnit>,
    /// The white space between its last base and its first level.
    tail: Range<usize>,
    levels: Vec<RubyLevel>,
}

impl Walk<'_> {
    /// Pushes the children of the ruby container `ruby`, pairing each annotation with a base of
    /// its own as CSS Ruby pairs them (CSS Ruby 1, section 2.3), one column at a time.
    ///
    /// The children form segments: bases, `<rb>`s or runs of other inline content, then
    /// annotation levels, `<rt>`s in a row or `<rtc>`s. The n-th annotation of each level goes
    /// over the n-th base, and an annotation past the last base over an empty one. White space
    /// between two bases pairs with the white space between the annotations above them, as a
    /// column of its own. A level that spans the bases puts them in an anonymous ruby container
    /// of their own. White space elsewhere stays where it stands, so a segment of one base and
    /// one annotation per level is pushed in tree order.
    fn ruby_children<B: InlineBuilder>(
        &mut self,
        builder: &mut B,
        ruby: &Node,
        text_transform: &CaseTransform,
    ) {
        let nodes = self.nodes;
        let mut ids: Vec<NodeId> = ruby.before().into_iter().collect();
        ids.extend(ruby.children.iter().copied());
        ids.extend(ruby.after());
        let tokens = self.ruby_tokens(&ids, 0..ids.len(), false);
        let mut segments: Vec<RubySegment> = Vec::new();
        let mut pending: Option<Range<usize>> = None;
        let mut in_levels = false;
        for (kind, range) in tokens {
            match kind {
                RubyChild::Space => pending = Some(range),
                RubyChild::Base | RubyChild::Inline => {
                    let unit = RubyUnit {
                        nodes: range,
                        anonymous: kind == RubyChild::Inline,
                        space: 0..0,
                    };
                    match segments.last_mut() {
                        Some(segment) if !in_levels => segment.bases.push(RubyUnit {
                            space: pending.take().unwrap_or_default(),
                            ..unit
                        }),
                        _ => segments.push(RubySegment {
                            lead: pending.take().unwrap_or_default(),
                            bases: vec![unit],
                            ..RubySegment::default()
                        }),
                    }
                    in_levels = false;
                }
                RubyChild::Annotation | RubyChild::Container => {
                    if segments.is_empty() {
                        segments.push(RubySegment {
                            lead: pending.take().unwrap_or_default(),
                            ..RubySegment::default()
                        });
                    }
                    let Some(segment) = segments.last_mut() else {
                        continue;
                    };
                    if !in_levels {
                        segment.tail = pending.take().unwrap_or_default();
                        in_levels = true;
                    }
                    // White space between annotations of a level pairs; between levels it goes.
                    let space = pending.take().unwrap_or_default();
                    if kind == RubyChild::Container {
                        let container = ids[range.start];
                        let level = self.container_level(&mut ids, container);
                        segment.levels.push(level);
                        continue;
                    }
                    let unit = RubyUnit {
                        nodes: range,
                        anonymous: false,
                        space: 0..0,
                    };
                    match segment.levels.last_mut() {
                        Some(level) if level.container.is_none() => {
                            level.units.push(RubyUnit { space, ..unit })
                        }
                        _ => segment.levels.push(RubyLevel {
                            container: None,
                            units: vec![unit],
                        }),
                    }
                }
                RubyChild::Nothing => {}
            }
        }
        for segment in &segments {
            self.ruby_segment(builder, &ids, segment, text_transform);
        }
        if let Some(trailing) = pending {
            for &id in &ids[trailing] {
                self.node(builder, &nodes[id], text_transform);
            }
        }
    }

    /// Returns the annotation level the `<rtc>` `container` makes, adding its children to `ids`.
    fn container_level(&self, ids: &mut Vec<NodeId>, container: NodeId) -> RubyLevel {
        let start = ids.len();
        ids.extend(self.nodes[container].children.iter().copied());
        let mut units: Vec<RubyUnit> = Vec::new();
        let mut space = None;
        for (kind, range) in self.ruby_tokens(ids, start..ids.len(), true) {
            match kind {
                RubyChild::Space => space = Some(range),
                RubyChild::Annotation | RubyChild::Inline => units.push(RubyUnit {
                    nodes: range,
                    anonymous: kind == RubyChild::Inline,
                    // White space at the container's start pairs with nothing.
                    space: space
                        .take()
                        .filter(|_| !units.is_empty())
                        .unwrap_or_default(),
                }),
                _ => {}
            }
        }
        // Without an `<rt>`, the container is one annotation over all the bases.
        if !units.iter().any(|unit| !unit.anonymous) {
            units.clear();
        }
        RubyLevel {
            container: Some(container),
            units,
        }
    }

    /// Splits `ids[range]` into ruby units: each `<rb>`, `<rt>` and `<rtc>` alone, and each run of
    /// other content, which is `Space` where it holds only collapsible white space. Inside an
    /// annotation container (`in_container`), only `<rt>`s stand alone.
    fn ruby_tokens(
        &self,
        ids: &[NodeId],
        range: Range<usize>,
        in_container: bool,
    ) -> Vec<(RubyChild, Range<usize>)> {
        let mut tokens: Vec<(RubyChild, Range<usize>)> = Vec::new();
        let mut run: Option<(RubyChild, Range<usize>)> = None;
        for index in range {
            let mut kind = self.ruby_child(&self.nodes[ids[index]]);
            if in_container && matches!(kind, RubyChild::Base | RubyChild::Container) {
                kind = RubyChild::Inline;
            }
            match (kind, &mut run) {
                (
                    RubyChild::Space | RubyChild::Inline | RubyChild::Nothing,
                    Some((run_kind, nodes)),
                ) => {
                    nodes.end = index + 1;
                    if kind == RubyChild::Inline {
                        *run_kind = RubyChild::Inline;
                    }
                }
                (RubyChild::Nothing, None) => {}
                (RubyChild::Space | RubyChild::Inline, None) => {
                    run = Some((kind, index..index + 1))
                }
                _ => {
                    tokens.extend(run.take());
                    tokens.push((kind, index..index + 1));
                }
            }
        }
        tokens.extend(run);
        tokens
    }

    /// Returns what `node`, a child of a ruby or annotation container, is to box fixup.
    fn ruby_child(&self, node: &Node) -> RubyChild {
        let element = match &node.data {
            NodeData::Element(element) | NodeData::AnonymousBlock(element) => element,
            NodeData::Text(_) if node.is_collapsible_whitespace_node() => return RubyChild::Space,
            NodeData::Text(_) => return RubyChild::Inline,
            NodeData::Comment { .. } | NodeData::Document(_) => return RubyChild::Nothing,
        };
        let tag = &element.name.local;
        let hidden =
            *tag == local_name!("input") && element.attr(local_name!("type")) == Some("hidden");
        let Some(style) = node
            .primary_styles()
            .filter(|_| !hidden && *tag != local_name!("rp"))
        else {
            return RubyChild::Nothing;
        };
        let display = node.display_style().unwrap_or(Display::inline());
        match (display.outside(), display.inside()) {
            (DisplayOutside::None, DisplayInside::None) => RubyChild::Nothing,
            (DisplayOutside::Inline, DisplayInside::Flow)
                if !style.slow_clone_position().is_absolutely_positioned() =>
            {
                if *tag == local_name!("rb") {
                    RubyChild::Base
                } else if *tag == local_name!("rt") {
                    RubyChild::Annotation
                } else if *tag == local_name!("rtc") {
                    RubyChild::Container
                } else {
                    RubyChild::Inline
                }
            }
            _ => RubyChild::Inline,
        }
    }

    /// Pushes the children `ids[range]`.
    fn ruby_nodes<B: InlineBuilder>(
        &mut self,
        builder: &mut B,
        ids: &[NodeId],
        range: Range<usize>,
        text_transform: &CaseTransform,
    ) {
        let nodes = self.nodes;
        for &id in &ids[range] {
            self.node(builder, &nodes[id], text_transform);
        }
    }

    /// Pushes `segment` one column at a time: each base with the annotations paired with it.
    fn ruby_segment<B: InlineBuilder>(
        &mut self,
        builder: &mut B,
        ids: &[NodeId],
        segment: &RubySegment,
        text_transform: &CaseTransform,
    ) {
        self.ruby_nodes(builder, ids, segment.lead.clone(), text_transform);
        let paired = || {
            segment
                .levels
                .iter()
                .filter(|level| !level.units.is_empty())
        };
        // Annotations that all span the bases make one column of them.
        let columns = match paired().map(|level| level.units.len()).max() {
            Some(most) => most.max(segment.bases.len()),
            None => 1,
        };
        // One column: in tree order, each level whole.
        if columns <= 1 {
            for base in &segment.bases {
                self.ruby_nodes(builder, ids, base.space.clone(), text_transform);
                self.ruby_nodes(builder, ids, base.nodes.clone(), text_transform);
            }
            self.ruby_nodes(builder, ids, segment.tail.clone(), text_transform);
            builder.start_ruby_column();
            for level in &segment.levels {
                self.ruby_annotation(builder, ids, level, level.units.first(), text_transform);
            }
            return;
        }
        // Annotations that span the bases go over an anonymous container of the columns.
        let spans = segment.levels.iter().any(|level| level.units.is_empty());
        if spans {
            builder.push_anonymous_span(None, SpanKind::Ruby);
        }
        for column in 0..columns {
            let base = segment.bases.get(column);
            let spaced = |level: &RubyLevel| {
                level
                    .units
                    .get(column)
                    .is_some_and(|unit| !unit.space.is_empty())
            };
            // White space before the column, paired across the levels as a column of its own.
            if column > 0
                && (base.is_some_and(|base| !base.space.is_empty()) || paired().any(spaced))
            {
                if let Some(base) = base {
                    self.ruby_nodes(builder, ids, base.space.clone(), text_transform);
                }
                builder.start_ruby_column();
                for level in paired() {
                    let space = level.units.get(column).map(|unit| unit.space.clone());
                    let container = level.container.map(|id| &self.nodes[id]);
                    let kind = SpanKind::Annotation {
                        in_container: container.is_some(),
                    };
                    builder.push_anonymous_span(container, kind);
                    self.ruby_nodes(builder, ids, space.unwrap_or_default(), text_transform);
                    builder.pop_span(kind);
                }
            }
            if let Some(base) = base {
                self.ruby_nodes(builder, ids, base.nodes.clone(), text_transform);
            }
            if column + 1 == segment.bases.len() {
                self.ruby_nodes(builder, ids, segment.tail.clone(), text_transform);
            }
            builder.start_ruby_column();
            for level in paired() {
                self.ruby_annotation(builder, ids, level, level.units.get(column), text_transform);
            }
        }
        if spans {
            builder.pop_span(SpanKind::Ruby);
            for level in segment.levels.iter().filter(|level| level.units.is_empty()) {
                self.ruby_annotation(builder, ids, level, None, text_transform);
            }
        }
    }

    /// Pushes `unit` as an annotation of `level`: an `<rt>` as itself, a run of other content in
    /// an anonymous annotation. With no unit, a level of `<rt>`s takes an empty anonymous
    /// annotation, and an `<rtc>` holding none is pushed whole.
    fn ruby_annotation<B: InlineBuilder>(
        &mut self,
        builder: &mut B,
        ids: &[NodeId],
        level: &RubyLevel,
        unit: Option<&RubyUnit>,
        text_transform: &CaseTransform,
    ) {
        let nodes = self.nodes;
        let container = level.container.map(|id| &nodes[id]);
        match unit {
            Some(unit) if !unit.anonymous => {
                self.ruby_nodes(builder, ids, unit.nodes.clone(), text_transform)
            }
            None if level.units.is_empty() => {
                if let Some(container) = container {
                    self.node(builder, container, text_transform);
                }
            }
            _ => {
                let kind = SpanKind::Annotation {
                    in_container: container.is_some(),
                };
                let text_transform = match container.and_then(|node| node.primary_styles()) {
                    Some(style) => case_transform::<B>(Some(&*style)),
                    None => text_transform.clone(),
                };
                builder.push_anonymous_span(container, kind);
                if let Some(unit) = unit {
                    self.ruby_nodes(builder, ids, unit.nodes.clone(), &text_transform);
                }
                builder.pop_span(kind);
            }
        }
    }
}
