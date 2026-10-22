//! Building an inline formatting context's Parley layout from the DOM, and laying out outside
//! list markers.

use blitz_traits::node_id::NodeId;
use parley::{
    Brush, FontContext, FontFamily, InlineBox, LayoutContext, TreeBuilder, WhiteSpaceCollapse,
};
use style::computed_values::white_space_collapse::T as StyloWhiteSpaceCollapse;
use style::properties::ComputedValues;
use style::values::computed::TextTransform;
use stylo_taffy::CopyStyleGetters;

use super::super::{TextBrush, TextContext, stylo_to_parley};
use super::source_map::SourceMapRecorder;
use super::text_transform::{CaseTransform, PushedText, TextTransformer};
use super::{MarkerLayout, TextLayout};
use crate::Node;
use crate::layout::inline::push_inline_content;
use crate::node::Marker;
use crate::text::{InlineBoxKind, InlineBuilder, MarkerEngine, SpanKind};

/// The families a list marker's bullet is set in.
const BULLET_FONT_FAMILY: &str = "Bullet, monospace, sans-serif";

impl<B: Brush> PushedText for TreeBuilder<'_, B> {
    fn text(&self) -> &str {
        TreeBuilder::text(self)
    }

    fn has_pending_whitespace(&self) -> bool {
        TreeBuilder::has_pending_whitespace(self)
    }
}

/// Builds the Parley layout of the inline formatting context rooted at
/// `inline_context_root_node_id`.
pub(super) fn build_inline_layout_into(
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
    let mut tree = layout_ctx.tree_builder(font_ctx, scale, true, &parley_style);
    if let Some(style) = root_node_style.as_deref() {
        tree.set_base_direction(stylo_to_parley::base_direction(
            style.clone_direction(),
            style.clone_unicode_bidi(),
        ));
    }
    drop(root_node_style);

    let mut builder = ParleyBuilder {
        tree,
        root_style: &parley_style,
        transformer: TextTransformer::default(),
        source_map: SourceMapRecorder::new(&mut text_layout.source_map),
    };
    push_inline_content(nodes, root_node, &mut builder);
    text_layout.text = builder.tree.build_into(&mut text_layout.layout);
}

/// A Parley tree builder, as the walk of the DOM pushes into it.
struct ParleyBuilder<'a, 'b> {
    tree: TreeBuilder<'b, TextBrush>,
    /// The style of the inline formatting context's root, which an inside list marker is set in.
    root_style: &'a parley::TextStyle<'static, 'static, TextBrush>,
    /// Applies `text-transform` to text before it is pushed to the tree.
    transformer: TextTransformer,
    /// Records where each text node's content is in the tree's text.
    source_map: SourceMapRecorder<'a>,
}

impl ParleyBuilder<'_, '_> {
    /// Pushes text that no text node holds.
    fn push_generated_text(&mut self, text: &str, collapsing: bool) {
        self.source_map
            .push_generated_text(&mut self.tree, text, collapsing);
    }

    /// Whether the root's style collapses white space, as an inside list marker's text does.
    fn root_is_collapsing(&self) -> bool {
        matches!(
            self.root_style.white_space_collapse,
            WhiteSpaceCollapse::Collapse | WhiteSpaceCollapse::PreserveBreaks
        )
    }
}

impl InlineBuilder for ParleyBuilder<'_, '_> {
    fn push_marker(&mut self, marker: &Marker) {
        match marker {
            // Bullet glyphs live in the bundled bullet font. The position-outside
            // path already asks for it; without the same span here a marker like
            // disclosure-closed (U+25B8) falls back to the element's own font and
            // renders as a missing glyph.
            Marker::Char(char) => {
                let mut marker_style = self.root_style.clone();
                marker_style.font_family = BULLET_FONT_FAMILY.into();
                self.tree.push_style_span(marker_style);
                self.push_generated_text(&format!("{char} "), self.root_is_collapsing());
                self.tree.pop_style_span();
            }
            Marker::String(str) => self.push_generated_text(str, self.root_is_collapsing()),
        }
        // The marker is a box of its own, so the content's words do not continue from it.
        self.transformer.word_break(&self.tree);
    }

    fn push_span(&mut self, node: &Node, style: &ComputedValues, kind: SpanKind) {
        match kind {
            // A `display: contents` element keeps only its white space handling.
            SpanKind::Contents => self.tree.push_style_modification_span(&[
                parley::StyleProperty::WhiteSpaceCollapse(stylo_to_parley::white_space_collapse(
                    style.clone_white_space_collapse(),
                )),
                parley::StyleProperty::TextWrapMode(stylo_to_parley::text_wrap_mode(
                    style.clone_text_wrap_mode(),
                )),
            ]),
            _ => self
                .tree
                .push_style_span(stylo_to_parley::style(node.id, style)),
        }
    }

    #[inline]
    fn pop_span(&mut self, _kind: SpanKind) {
        self.tree.pop_style_span();
    }

    #[inline]
    fn push_text(&mut self, node: &Node, text: &str, style: Option<&ComputedValues>) {
        let text_transform = style.map_or(TextTransform::NONE, |s| s.clone_text_transform());
        let pushed = match style {
            Some(style) if !text_transform.is_empty() => {
                let transform = CaseTransform::from_style(style);
                self.transformer.transform(text, &transform, &self.tree)
            }
            _ => text,
        };
        self.source_map.push_text(
            &mut self.tree,
            node.id,
            text,
            pushed,
            text_transform,
            style.map_or(StyloWhiteSpaceCollapse::Collapse, |s| {
                s.clone_white_space_collapse()
            }),
        );
    }

    fn push_inline_box(&mut self, node: &Node, style: &ComputedValues, kind: InlineBoxKind) {
        if kind == InlineBoxKind::Atomic {
            self.transformer.word_break(&self.tree);
        }
        let inline_box = InlineBox {
            id: node.id.as_u64(),
            kind: match kind {
                InlineBoxKind::Atomic => parley::InlineBoxKind::InFlow,
                InlineBoxKind::Float => parley::InlineBoxKind::CustomOutOfFlow,
                InlineBoxKind::Absolute => parley::InlineBoxKind::OutOfFlow,
            },
            // Overridden by push_inline_box method
            index: 0,
            // Width and height are set during layout
            width: 0.0,
            height: 0.0,
            baseline: None,
            vertical_align: stylo_to_parley::vertical_align(style),
        };
        self.source_map.push_inline_box(&mut self.tree, inline_box);
    }

    fn push_line_break(&mut self, _node: &Node, _style: &ComputedValues) {
        self.tree
            .push_style_modification_span(&[parley::StyleProperty::WhiteSpaceCollapse(
                WhiteSpaceCollapse::Preserve,
            )]);
        self.push_generated_text("\n", false);
        self.tree.pop_style_span();
        self.transformer.word_break(&self.tree);
    }

    #[inline]
    fn push_break_opportunity(&mut self, _node: &Node) {
        // TODO: `<wbr>` is not implemented.
    }
}

impl MarkerEngine for MarkerLayout {
    fn build(
        cx: &mut TextContext,
        node: NodeId,
        style: &ComputedValues,
        marker: &Marker,
        bullet: bool,
        scale: f32,
    ) -> Self {
        let mut parley_style = stylo_to_parley::style(node, style);

        // Override the font to our specific bullet font when rendering bullets
        if bullet {
            parley_style.font_family = FontFamily::from(BULLET_FONT_FAMILY);
        }

        // Create a parley tree builder
        let mut font_ctx = cx.font_ctx.lock().unwrap();
        let mut builder = cx
            .layout_ctx
            .tree_builder(&mut font_ctx, scale, true, &parley_style);

        match marker {
            Marker::Char(char) => {
                let mut buf = [0u8; 4];
                builder.push_text(char.encode_utf8(&mut buf));
            }
            Marker::String(str) => builder.push_text(str),
        };

        let mut layout = builder.build().0;
        let width = layout.calculate_content_widths().max;
        layout.break_all_lines(Some(width));
        layout
    }
}
