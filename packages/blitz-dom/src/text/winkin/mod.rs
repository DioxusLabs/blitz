//! The winkin text backend.
//!
//! The content of an inline formatting context is built from the DOM the
//! first time it is laid out, and again whenever it changes or its atomic
//! inlines measure differently: winkin takes each atomic inline's size as it
//! is pushed, and Blitz only knows it once taffy has measured the box.

mod editor;
mod fonts;
pub(crate) mod style;

use ::style::context::{CascadeInputs, TreeCountingCaches};
use ::style::properties::{ComputedValues, FirstLineReparenting};
use ::style::rule_cache::RuleCacheConditions;
use ::style::selector_parser::PseudoElement;
use ::style::servo_arc::Arc as ServoArc;
use ::style::shared_lock::StylesheetGuards;
use ::style::stylist::Stylist;
use ::style::values::computed::Float;
use ::style::values::specified::box_::{DisplayInside, DisplayOutside};
use ::style::values::specified::position::PositionTryFallbacksTryTactic;
use blitz_traits::node_id::NodeId;
use fontwich::{Collection, LayerBuilder};
use kurbo::Affine;
use markup5ever::{local_name, ns};
use winkin::style::{
    BaseDirection, BidiGroup, EdgesGroup, FontFamilyName, GenericFamily, Language, LineGroup,
    UnicodeBidi, VerticalAlign, WritingMode,
};
use winkin::{
    Area, BlockExtents, BoxSize, BuildOptions, ComputedBlockStyle, ComputedStyle, Context,
    Exclusions, ExclusionsCheckpoint, FloatSide, InlineExtents, IntrinsicSizes, Item, Layout,
    LayoutBuilder, LineMetrics, NodeKey, OriginalDisplay, PlacedFloat,
};

use super::{
    ContentWidths, FloatRequest, InlineBoxKind, InlineBuilder, InlineLayoutEngine, InlineText,
    LastBaseline, LineArea, LineExclusions, LineFlow, LinesExtent, LinesInputs, MarkerEngine,
    Placement, SpanKind,
};
use crate::layout::inline::push_inline_content;
use crate::layout::text_transform::PushedText;
use crate::node::{InlineContent, InlineTextHit, Marker};
use crate::{BaseDocument, Node, NodeData};

/// The fontwich crate, for the renderer and for hosts that build fonts.
pub use ::fontwich;
/// The winkin crate, for the renderer's winkin painter.
pub use ::winkin;
pub use editor::TextEditor;
pub use fonts::FontContext;

/// An outside list marker, laid out by winkin.
pub type MarkerLayout = TextLayout;

/// The key of a block's `::first-letter` box, which has no node of its own:
/// the block's node with this bit set.
pub const FIRST_LETTER_KEY: u64 = 1 << 62;

/// The key of a block's inside list marker, which has no node of its own:
/// the block's node with this bit set.
pub const MARKER_KEY: u64 = 1 << 61;

/// What a document keeps for its text under winkin: the fonts winkin chooses from, and the
/// context it builds and breaks in.
///
/// The platform's fonts and the fonts Blitz ships are the collection's
/// lower layers; a document's `@font-face` fonts are a layer of their own
/// above them, which a face joins as it loads, the context being handed
/// the new collection.
pub struct TextContext {
    /// What every layout is built and broken with.
    pub(crate) cx: Context,
    /// The fonts the document was handed, which its iframes are handed too.
    given: FontContext,
    /// The installed and shipped fonts.
    base: Collection,
    /// The document's `@font-face` faces.
    document: LayerBuilder,
    /// The collection the context has, which font metrics are read from.
    metrics: std::sync::Arc<std::sync::RwLock<Collection>>,
}

/// An inline formatting context laid out by winkin.
#[derive(Default)]
pub struct TextLayout {
    /// The content, and its lines once they are broken. Its allocations are
    /// reused by the next build.
    layout: Layout,
    /// Whether the layout holds content built for the node as it is now.
    built: bool,
    /// Whether the content has been broken into lines since it was built.
    laid: bool,
    /// What the atomic inlines and floats were built at, in content order,
    /// and what percentages of the containing block were resolved against.
    boxes: Vec<BuiltBox>,
    basis: f32,
    /// The computed styles painted with that no node of the content's is
    /// styled in.
    styles: PaintStyles,
    /// Which way the content's lines run.
    writing_mode: WritingMode,
    /// Device pixels per CSS pixel used to build the layout.
    scale: f32,
    /// Block-axis offset (in CSS px) of the line boxes from the top of the
    /// container's content box, as applied by `align-content`.
    pub(crate) block_offset: f32,
    /// The size of the content box the lines were last placed in, in device pixels.
    content_size: kurbo::Size,
}

/// A copy starts empty, and is built again the first time it is laid out.
impl Clone for TextLayout {
    fn clone(&self) -> Self {
        Self::default()
    }
}

/// The computed styles a context's text is painted with where they are not
/// its nodes' own.
#[derive(Default)]
struct PaintStyles {
    /// Each element's computed style on the first formatted line, by node,
    /// where the block has a `::first-line`. The block's own is its
    /// `::first-line` style.
    first_line: Vec<(u64, ServoArc<ComputedValues>)>,
    /// The `::first-letter` box's style, and its style on the first line
    /// where the block has a `::first-line`.
    first_letter: Option<(ServoArc<ComputedValues>, Option<ServoArc<ComputedValues>>)>,
    /// The inside list marker's style.
    marker: Option<ServoArc<ComputedValues>>,
    /// Each element's emphasis mark, by node, where its text takes marks:
    /// the mark's string laid out on one line in the element's fonts at the
    /// marks' size, which a mark is drawn as.
    marks: Vec<(u64, Layout)>,
}

/// An atomic inline or a float as the content was built with it.
#[derive(Copy, Clone, PartialEq, Debug)]
pub(crate) struct BuiltBox {
    pub(crate) node: u64,
    /// Its border box in device pixels, and its baseline where it has one.
    pub(crate) size: BoxSize,
}

impl std::fmt::Debug for TextLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TextLayout")
    }
}

impl TextLayout {
    /// Drops the built content, keeping its allocations, so that it is built
    /// again.
    pub(crate) fn invalidate(&mut self) {
        self.built = false;
        self.laid = false;
    }

    /// Whether the content, as it was last built, has lines that run down the
    /// page.
    pub(crate) fn is_vertical(&self) -> bool {
        self.writing_mode != WritingMode::HorizontalTb
    }

    /// Which way the content's lines run, as it was last built.
    pub fn writing_mode(&self) -> WritingMode {
        self.writing_mode
    }

    /// The computed style `node` is set in on the block's first formatted
    /// line, where `::first-line` restyles it: the block's own is its
    /// `::first-line` style.
    pub fn first_line_style_of(&self, node: u64) -> Option<ServoArc<ComputedValues>> {
        self.first_line_style(node).cloned()
    }

    /// Returns the last layout, where there is one.
    pub fn layout(&self) -> Option<&Layout> {
        self.laid.then_some(&self.layout)
    }

    /// The emphasis mark the text of element `node` is marked with, laid
    /// out on one line at the marks' size, where its text takes marks.
    pub fn emphasis_mark(&self, node: u64) -> Option<&Layout> {
        self.styles
            .marks
            .iter()
            .find(|(held, _)| *held == node)
            .map(|(_, layout)| layout)
    }

    /// The computed style the text and boxes of `key` are painted in, on the
    /// block's first formatted line where `first_line`.
    ///
    /// A text node's is the element it is in. The block's `::first-letter`
    /// box and its inside list marker have styles of their own, and on the
    /// first line every element is set in its style reparented onto its
    /// parent's `::first-line` style.
    pub fn computed_style(
        &self,
        doc: &BaseDocument,
        key: u64,
        first_line: bool,
    ) -> Option<ServoArc<ComputedValues>> {
        if key & FIRST_LETTER_KEY != 0 {
            let (letter, on_first_line) = self.styles.first_letter.as_ref()?;
            return Some(
                on_first_line
                    .as_ref()
                    .filter(|_| first_line)
                    .unwrap_or(letter)
                    .clone(),
            );
        }
        if key & MARKER_KEY != 0 {
            let node = key & !MARKER_KEY;
            if first_line && let Some(style) = self.first_line_style(node) {
                return Some(style.clone());
            }
            return self.styles.marker.clone();
        }
        let node = doc.get_node(NodeId::from_u64(key))?;
        let styled = match &node.data {
            NodeData::Text(_) => doc.get_node(node.parent?)?,
            _ => node,
        };
        if first_line && let Some(style) = self.first_line_style(styled.id.as_u64()) {
            return Some(style.clone());
        }
        styled.primary_styles().map(|style| (*style).clone())
    }

    /// The computed style `node` is set in on the first line, if
    /// `::first-line` restyles it.
    fn first_line_style(&self, node: u64) -> Option<&ServoArc<ComputedValues>> {
        self.styles
            .first_line
            .iter()
            .find(|(held, _)| *held == node)
            .map(|(_, style)| style)
    }

    /// Whether the content is built, with atomic inlines and floats
    /// measured as `sizes` are and percentages taken of `basis`.
    pub(crate) fn is_built_with(&self, sizes: &[BuiltBox], basis: f32) -> bool {
        self.built && self.boxes == sizes && self.basis == basis
    }

    /// Returns the content's min-content and max-content widths.
    pub(crate) fn intrinsic_widths(&self) -> IntrinsicSizes {
        if self.built {
            self.layout.intrinsic_sizes()
        } else {
            IntrinsicSizes::default()
        }
    }

    /// Breaks the content into lines in `area`, in the room `exclusions`
    /// leaves, and returns the layout.
    pub(crate) fn lay_out(
        &mut self,
        cx: &mut Context,
        area: Area,
        exclusions: &mut dyn Exclusions,
    ) -> Option<&Layout> {
        if !self.built {
            return None;
        }
        self.layout.break_lines(cx, area, exclusions);
        self.laid = true;
        Some(&self.layout)
    }
}

/// What restyles an element for the first formatted line, and a block's
/// `::first-letter` for the box its letter is in.
#[derive(Copy, Clone)]
pub(crate) struct Cascade<'a> {
    pub(crate) stylist: &'a Stylist,
    pub(crate) guards: &'a StylesheetGuards<'a>,
}

impl Cascade<'_> {
    /// `style`, the computed style of `node`, with its inherited properties
    /// taken from `parent` instead of its parent's style.
    ///
    /// This is what an element on the first formatted line is set in: it
    /// inherits from its parent's `::first-line` style, and keeps its own
    /// reset properties.
    fn reparent(
        self,
        node: &Node,
        style: &ComputedValues,
        parent: &ComputedValues,
    ) -> ServoArc<ComputedValues> {
        self.stylist.cascade_style_and_visited(
            Some(node),
            None,
            &CascadeInputs::new_from_style(style),
            self.guards,
            Some(parent),
            Some(parent),
            FirstLineReparenting::Yes {
                style_to_reparent: style,
            },
            &PositionTryFallbacksTryTactic::default(),
            None,
            &mut RuleCacheConditions::default(),
            &mut TreeCountingCaches::default(),
        )
    }

    /// The block `block`'s `::first-letter` style, `letter`, cascaded again
    /// to inherit from `parent`, the style of the box the letter is in.
    ///
    /// Stylo cascades the pseudo-element against the block, where CSS has it
    /// inherit from the innermost box its text is in.
    fn first_letter(
        self,
        block: &Node,
        letter: &ComputedValues,
        parent: &ComputedValues,
    ) -> ServoArc<ComputedValues> {
        self.stylist.cascade_style_and_visited(
            Some(block),
            Some(&PseudoElement::FirstLetter),
            &CascadeInputs::new_from_style(letter),
            self.guards,
            Some(parent),
            Some(parent),
            FirstLineReparenting::No,
            &PositionTryFallbacksTryTactic::default(),
            None,
            &mut RuleCacheConditions::default(),
            &mut TreeCountingCaches::default(),
        )
    }
}

/// The content language of `node`: its nearest `lang`, its own or an
/// ancestor's. An empty `lang` is no language.
fn language_of(nodes: &crate::NodeTree, mut node: Option<NodeId>) -> Option<Language> {
    while let Some(id) = node {
        let held = &nodes[id];
        if let Some(language) = own_language(held) {
            return language;
        }
        node = held.parent;
    }
    None
}

/// What `node`'s own `lang` says, where it has one: `Some(None)` for an
/// empty or unreadable one, which is no language.
fn own_language(node: &Node) -> Option<Option<Language>> {
    let element = node.element_data()?;
    let lang = element.attrs.iter().find(|attr| {
        attr.name.local == local_name!("lang")
            && (attr.name.ns == ns!() || attr.name.ns == ns!(xml))
    })?;
    Some(Language::parse(lang.value.trim()).ok())
}

/// The font families a list marker's bullet is set in before the block's
/// own: Blitz's bullet font, which Parley's list markers ask for too.
const BULLET_FAMILIES: [FontFamilyName<'static>; 2] = [
    FontFamilyName::Named(std::borrow::Cow::Borrowed("Bullet")),
    FontFamilyName::Generic(GenericFamily::Monospace),
];

/// Builds the content of the inline formatting context rooted at `root_id`.
///
/// `sizes` are the atomic inlines and floats as taffy measured them, in
/// device pixels, and `basis` is the containing block's inline size in CSS
/// pixels, which percentages of it are taken of.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build(
    nodes: &crate::NodeTree,
    cascade: Cascade<'_>,
    cx: &mut Context,
    text: &mut TextLayout,
    scale: f32,
    basis: f32,
    root_id: NodeId,
    sizes: &[BuiltBox],
) {
    let root = &nodes[root_id];
    let key = NodeKey(root_id.as_u64());
    text.built = false;
    text.laid = false;
    text.boxes.clear();
    text.boxes.extend_from_slice(sizes);
    text.basis = basis;
    text.scale = scale;
    text.styles.first_line.clear();
    text.styles.first_letter = None;
    text.styles.marker = None;
    text.styles.marks.clear();

    // An anonymous block has no style of its own worth reading: its text is
    // set in the box around it.
    let root_computed = root
        .primary_styles()
        .map(|style| (*style).clone())
        .or_else(|| {
            root.parent
                .and_then(|parent| nodes[parent].primary_styles().map(|style| (*style).clone()))
        });
    let Some(root_computed) = root_computed else {
        // Nothing styled: an empty layout, so that reading it reads nothing.
        let initial = ComputedStyle::initial();
        text.layout
            .builder(
                key,
                &ComputedBlockStyle::new(&initial),
                BuildOptions::default(),
            )
            .finish(cx);
        text.built = true;
        return;
    };
    let container = match &root.data {
        NodeData::AnonymousBlock(_) => root
            .parent
            .and_then(|parent| nodes[parent].primary_styles().map(|style| (*style).clone()))
            .unwrap_or_else(|| root_computed.clone()),
        _ => root_computed.clone(),
    };
    let language = language_of(nodes, Some(root_id));
    let feature_values = style::FeatureValues::of(cascade.stylist);
    let root_lists = style::FontLists::of(&root_computed, &feature_values);
    let mut root_style = style::computed_style(&root_lists, &root_computed, scale, basis, language);
    // An anonymous block's lines are the element's around it, whose
    // `unicode-bidi` applies to them: an override, or `plaintext`.
    if matches!(root.data, NodeData::AnonymousBlock(_)) {
        root_style.bidi.unicode_bidi = style::unicode_bidi(&container);
    }
    // The block's `::first-line`, which the first line's elements inherit
    // from.
    let first_line_computed = root
        .try_stylo_element_data()
        .and_then(|data| data.get())
        .and_then(|data| data.styles.pseudos.get(&PseudoElement::FirstLine).cloned());
    let first_line_lists = first_line_computed
        .as_deref()
        .map(|computed| style::FontLists::of(computed, &feature_values));
    let first_line_style = match (&first_line_computed, &first_line_lists) {
        (Some(computed), Some(lists)) => Some(style::computed_style(
            lists, computed, scale, basis, language,
        )),
        _ => None,
    };
    let first_letter_computed = root
        .try_stylo_element_data()
        .and_then(|data| data.get())
        .and_then(|data| {
            data.styles
                .pseudos
                .get(&PseudoElement::FirstLetter)
                .cloned()
        });
    if let Some(computed) = &first_line_computed {
        text.styles
            .first_line
            .push((root_id.as_u64(), computed.clone()));
    }

    let mut block = style::block_style(
        &root_style,
        first_line_style.as_ref(),
        &root_computed,
        &container,
        scale,
    );
    // The root element's lines run in the body's direction, which CSS
    // Writing Modes propagates to the root as its used value.
    let element = match &root.data {
        NodeData::AnonymousBlock(_) => root.parent,
        _ => Some(root_id),
    };
    if block.direction != BaseDirection::Auto
        && let Some(rtl) = element.and_then(|element| body_is_rtl(nodes, element))
    {
        block.direction = if rtl {
            BaseDirection::Rtl
        } else {
            BaseDirection::Ltr
        };
    }
    // `text-indent` indents the element's first formatted line, which an
    // anonymous block after another box of the element's does not hold.
    if matches!(root.data, NodeData::AnonymousBlock(_))
        && !block.text_indent.hanging
        && follows_a_box(nodes, root)
    {
        block.text_indent.amount = winkin::style::LengthPercentage::default();
    }
    text.writing_mode = block.writing_mode;

    let TextLayout { layout, styles, .. } = text;
    let mut options = BuildOptions::default();
    // Hit testing needs to map a layout position back to its DOM node.
    options.map_source = true;
    let mut builder = WinkinBuilder {
        builder: layout.builder(key, &block, options),
        cascade,
        root,
        boxes: sizes,
        styles,
        first_letter: first_letter_computed,
        feature_values,
        marked: Vec::new(),
        scale,
        basis,
        parents: vec![Parent {
            computed: root_computed,
            first_line: first_line_computed,
            language,
            is_root: true,
        }],
    };
    push_inline_content(nodes, root, &mut builder);
    let WinkinBuilder {
        builder,
        marked,
        feature_values,
        styles,
        ..
    } = builder;
    builder.finish(cx);
    for (node, computed, language) in marked {
        if let Some(layout) = emphasis_mark(cx, &computed, &feature_values, scale, basis, language)
        {
            styles.marks.push((node, layout));
        }
    }
    text.built = true;
}

/// The emphasis mark of text set in `computed`, laid out on one line: the
/// style's mark string in its fonts at half its size, rounded to a pixel,
/// as Chrome draws the string in the text's font at that size.
fn emphasis_mark(
    cx: &mut Context,
    computed: &ComputedValues,
    feature_values: &style::FeatureValues,
    scale: f32,
    basis: f32,
    language: Option<Language>,
) -> Option<Layout> {
    let string = style::emphasis_mark_string(computed)?;
    let lists = style::FontLists::of(computed, feature_values);
    let own = unboxed(style::computed_style(
        &lists, computed, scale, basis, language,
    ));
    let mark = ComputedStyle {
        font: winkin::style::FontGroup {
            size: (own.font.size / 2.0).round(),
            ..own.font
        },
        text: winkin::style::TextGroup {
            language: own.text.language,
            white_space_collapse: winkin::style::WhiteSpaceCollapse::Preserve,
            wrap_mode: winkin::style::TextWrapMode::NoWrap,
            ..winkin::style::TextGroup::INITIAL
        },
        line: LineGroup::INITIAL,
        decorates: false,
        ..own
    };
    let block = ComputedBlockStyle {
        writing_mode: WritingMode::HorizontalTb,
        direction: BaseDirection::Ltr,
        ..ComputedBlockStyle::new(&mark)
    };
    let mut layout = Layout::default();
    let key = NodeKey(0);
    let mut builder = layout.builder(key, &block, BuildOptions::default());
    builder.text(key, &string);
    builder.finish(cx);
    layout.break_lines(cx, Area::new(f32::MAX / 4.0), &mut winkin::NoExclusions);
    Some(layout)
}

/// `style` as the style of a box that has no box: no edges, nothing
/// painted, and the properties that do not inherit at their initial
/// values, so that the text inside it is set in `style` and nothing else of
/// it shows.
fn unboxed(style: ComputedStyle<'_>) -> ComputedStyle<'_> {
    ComputedStyle {
        edges: EdgesGroup::INITIAL,
        paints: false,
        line: LineGroup {
            vertical_align: VerticalAlign::Baseline,
            ..style.line
        },
        bidi: BidiGroup {
            unicode_bidi: UnicodeBidi::Normal,
            ..style.bidi
        },
        ..style
    }
}

/// What a box's children are set in: the box around them.
struct Parent {
    computed: ServoArc<ComputedValues>,
    /// Its style on the first formatted line, where the block has a
    /// `::first-line`.
    first_line: Option<ServoArc<ComputedValues>>,
    language: Option<Language>,
    /// Whether it is the block itself.
    is_root: bool,
}

/// A winkin layout builder, as the walk of the DOM pushes into it.
struct WinkinBuilder<'a, 'b> {
    builder: LayoutBuilder<'b>,
    cascade: Cascade<'a>,
    /// The block the content is for.
    root: &'a Node,
    boxes: &'a [BuiltBox],
    styles: &'b mut PaintStyles,
    /// The block's `::first-letter` style, until its letter is found.
    first_letter: Option<ServoArc<ComputedValues>>,
    /// The document's `@font-feature-values` rules.
    feature_values: style::FeatureValues,
    /// The elements whose text takes emphasis marks, in the styles and
    /// languages their marks are set in.
    marked: Vec<(u64, ServoArc<ComputedValues>, Option<Language>)>,
    scale: f32,
    basis: f32,
    /// The boxes open around what is pushed next, the block's first.
    parents: Vec<Parent>,
}

impl WinkinBuilder<'_, '_> {
    /// The box what is pushed next is in.
    fn parent(&self) -> &Parent {
        // The block itself is never popped.
        &self.parents[self.parents.len() - 1]
    }

    /// The size taffy measured `node` at.
    fn measured(&self, node: u64) -> BoxSize {
        self.boxes
            .iter()
            .find(|size| size.node == node)
            .map_or(BoxSize::default(), |size| size.size)
    }

    /// The style of an atomic inline or a float, `computed`, as winkin sets
    /// it: where it sits on the line and its margins. Its border and padding
    /// are inside the border box taffy measured, and taffy paints it.
    fn boxed_style<'s>(
        &self,
        lists: &'s style::FontLists,
        computed: &'s ComputedValues,
        language: Option<Language>,
    ) -> ComputedStyle<'s> {
        let own = style::computed_style(lists, computed, self.scale, self.basis, language);
        ComputedStyle {
            edges: EdgesGroup {
                border: EdgesGroup::INITIAL.border,
                padding: EdgesGroup::INITIAL.padding,
                ..own.edges
            },
            paints: false,
            ..own
        }
    }

    /// Asks for the block's `::first-letter` before `text`, in the style the
    /// pseudo-element has in the box the text is in, until the letter is
    /// found.
    ///
    /// The builder finds the letter as the text is written; it is found once
    /// a text holds anything but white space and punctuation, and asking
    /// again after is ignored, so the asking stops there.
    fn arm_first_letter(&mut self, text: &str) {
        let Some(pseudo) = &self.first_letter else {
            return;
        };
        let parent = self.parent();
        let letter = if parent.is_root {
            pseudo.clone()
        } else {
            self.cascade
                .first_letter(self.root, pseudo, &parent.computed)
        };
        let letter_first_line = parent
            .first_line
            .as_ref()
            .map(|first_line| self.cascade.first_letter(self.root, pseudo, first_line));
        let language = parent.language;
        {
            let lists = style::FontLists::of(&letter, &self.feature_values);
            let style = style::computed_style(&lists, &letter, self.scale, self.basis, language);
            let first_line_lists = letter_first_line
                .as_deref()
                .map(|computed| style::FontLists::of(computed, &self.feature_values));
            let first_line_style = match (&letter_first_line, &first_line_lists) {
                (Some(computed), Some(lists)) => Some(style::computed_style(
                    lists, computed, self.scale, self.basis, language,
                )),
                _ => None,
            };
            self.builder.set_first_letter(
                NodeKey(FIRST_LETTER_KEY | self.root.id.as_u64()),
                &style,
                first_line_style.as_ref(),
            );
        }
        self.styles.first_letter = Some((letter, letter_first_line));
        if text
            .chars()
            .any(|ch| !ch.is_whitespace() && !is_punctuation(ch))
        {
            self.first_letter = None;
        }
    }
}

/// winkin reads no text back: it applies `text-transform` itself.
impl PushedText for WinkinBuilder<'_, '_> {
    fn text(&self) -> &str {
        ""
    }

    fn has_pending_whitespace(&self) -> bool {
        false
    }
}

impl InlineBuilder for WinkinBuilder<'_, '_> {
    const TRANSFORMS_TEXT: bool = true;
    const SETS_RUBY: bool = true;

    /// A marker inside the list item comes before its content, in the bullet
    /// font where it is a bullet.
    fn push_marker(&mut self, marker: &Marker) {
        let root_id = self.root.id;
        let marker_key = NodeKey(MARKER_KEY | root_id.as_u64());
        let root = &self.parents[0];
        let root_computed = root.computed.clone();
        let language = root.language;
        self.styles.marker = Some(root_computed.clone());
        match marker {
            Marker::Char(char) => {
                let lists = style::FontLists::of(&root_computed, &self.feature_values)
                    .with_families_first(&BULLET_FAMILIES);
                let bullet = unboxed(style::computed_style(
                    &lists,
                    &root_computed,
                    self.scale,
                    self.basis,
                    language,
                ));
                self.builder.open_box(marker_key, &bullet, None);
                self.builder.text(marker_key, &format!("{char} "));
                self.builder.close_box();
            }
            Marker::String(string) => self.builder.text(marker_key, string),
        }
    }

    /// Opens an inline box -- a span, a ruby container, an annotation, or
    /// the box a `display: contents` element's children inherit from.
    fn push_span(&mut self, node: &Node, _style: &ComputedValues, kind: SpanKind) {
        let Some(computed) = node.primary_styles().map(|style| (*style).clone()) else {
            return;
        };
        let parent = self.parent();
        let language = own_language(node).unwrap_or(parent.language);
        let key = NodeKey(node.id.as_u64());
        // On the first line, the same element inheriting from its parent's
        // `::first-line` instead.
        let first_line = parent
            .first_line
            .as_ref()
            .map(|first_line| self.cascade.reparent(node, &computed, first_line));
        // An `<rt>` in an `<rtc>` takes the side of its container, which
        // `ruby-position` applies to.
        let position = match kind {
            SpanKind::Annotation { in_container: true } => {
                Some(style::ruby_position(&parent.computed))
            }
            _ => None,
        };
        if kind == SpanKind::Ruby {
            self.first_letter = None;
        }
        if let Some(style) = &first_line {
            self.styles
                .first_line
                .push((node.id.as_u64(), style.clone()));
        }
        {
            let lists = style::FontLists::of(&computed, &self.feature_values);
            let own = style::computed_style(&lists, &computed, self.scale, self.basis, language);
            let first_line_lists = first_line
                .as_deref()
                .map(|computed| style::FontLists::of(computed, &self.feature_values));
            let first_line_style = match (&first_line, &first_line_lists) {
                (Some(computed), Some(lists)) => Some(style::computed_style(
                    lists, computed, self.scale, self.basis, language,
                )),
                _ => None,
            };
            let builder = &mut self.builder;
            match (kind, position) {
                (SpanKind::Span, _) => builder.open_box(key, &own, first_line_style.as_ref()),
                (SpanKind::Ruby, _) => builder.open_ruby(key, &own, first_line_style.as_ref()),
                (SpanKind::Annotation { .. }, Some(position)) => builder
                    .open_annotation_with_position(key, &own, first_line_style.as_ref(), position),
                (SpanKind::Annotation { .. }, None) => {
                    builder.open_annotation(key, &own, first_line_style.as_ref())
                }
                // No box, but its children inherit from it: a box with no
                // edges and nothing painted, in its style.
                (SpanKind::Contents, _) => {
                    let first_line_style = first_line_style.map(unboxed);
                    builder.open_box(key, &unboxed(own), first_line_style.as_ref());
                }
            }
        }
        self.parents.push(Parent {
            computed,
            first_line,
            language,
            is_root: false,
        });
    }

    fn pop_span(&mut self, kind: SpanKind) {
        if self.parents.len() > 1 {
            self.parents.pop();
        }
        match kind {
            SpanKind::Span | SpanKind::Contents => self.builder.close_box(),
            SpanKind::Ruby => self.builder.close_ruby(),
            SpanKind::Annotation { .. } => self.builder.close_annotation(),
        }
    }

    /// A text node carries no style of its own: the box it is in is what sets
    /// it, and a painter reads the element it is in.
    fn push_text(&mut self, node: &Node, text: &str) {
        if text.is_empty() {
            return;
        }
        self.arm_first_letter(text);
        let parent = self.parent();
        if let Some(element) = node.parent
            && style::emphasis_mark_string(&parent.computed).is_some()
            && !self
                .marked
                .iter()
                .any(|(held, ..)| *held == element.as_u64())
        {
            let marked = (element.as_u64(), parent.computed.clone(), parent.language);
            self.marked.push(marked);
        }
        self.builder.text(NodeKey(node.id.as_u64()), text);
    }

    /// Pushes an atomic inline or a float, sized as taffy measured it, or the
    /// anchor of an absolutely positioned box, which takes no room and whose
    /// static position the lines find.
    fn push_inline_box(&mut self, node: &Node, computed: &ComputedValues, kind: InlineBoxKind) {
        let key = NodeKey(node.id.as_u64());
        let language = own_language(node).unwrap_or(self.parent().language);
        match kind {
            InlineBoxKind::Absolute => {
                let display =
                    if computed.get_box().original_display.outside() == DisplayOutside::Inline {
                        OriginalDisplay::Inline
                    } else {
                        OriginalDisplay::Block
                    };
                self.builder.absolute(key, display);
            }
            InlineBoxKind::Atomic => {
                // A block has one first letter, and it comes before any atomic
                // inline.
                self.first_letter = None;
                let lists = style::FontLists::of(computed, &self.feature_values);
                let own = self.boxed_style(&lists, computed, language);
                let size = self.measured(node.id.as_u64());
                self.builder.atomic(key, &own, None, size);
            }
            InlineBoxKind::Float => {
                // `inline-start` and `inline-end` are the containing block's
                // sides, as its direction has them.
                let rtl = self.root.primary_styles().is_some_and(|block| {
                    block.get_inherited_box().direction
                        == ::style::computed_values::direction::T::Rtl
                });
                let side = match (computed.clone_float(), rtl) {
                    (Float::Right, _) | (Float::InlineStart, true) | (Float::InlineEnd, false) => {
                        FloatSide::Right
                    }
                    _ => FloatSide::Left,
                };
                let lists = style::FontLists::of(computed, &self.feature_values);
                let own = self.boxed_style(&lists, computed, language);
                let size = self.measured(node.id.as_u64());
                self.builder.float(key, &own, side, size);
            }
        }
    }

    fn push_line_break(&mut self, node: &Node, computed: &ComputedValues) {
        self.first_letter = None;
        let key = NodeKey(node.id.as_u64());
        match node
            .element_data()
            .and_then(|element| br_clear(computed, element))
        {
            Some(clear) => self.builder.line_break_clearing(key, clear),
            None => self.builder.line_break(key),
        }
    }

    fn push_break_opportunity(&mut self, _node: &Node) -> bool {
        self.builder.break_opportunity();
        true
    }
}

/// Whether `ch` is punctuation, which a first letter may take before it
/// and goes on looking past.
fn is_punctuation(ch: char) -> bool {
    ch.is_ascii_punctuation()
        || matches!(
            ch,
            '\u{00A1}' | '\u{00A7}' | '\u{00AB}' | '\u{00B6}' | '\u{00B7}' | '\u{00BB}' | '\u{00BF}'
                | '\u{2010}'..='\u{2027}'
                | '\u{2030}'..='\u{205E}'
                | '\u{3001}'..='\u{3003}'
                | '\u{3008}'..='\u{3011}'
                | '\u{3014}'..='\u{301F}'
                | '\u{FF01}'..='\u{FF0F}'
        )
}

/// Whether `node` comes after another box among its parent's layout
/// children, where the parent is a block container: one in the flow, not
/// floated or absolutely positioned. A flex or grid item is a block of
/// its own, with a first line of its own.
fn follows_a_box(nodes: &crate::NodeTree, node: &Node) -> bool {
    let Some(parent) = node.parent else {
        return false;
    };
    let in_block_container = nodes[parent].display_style().is_some_and(|display| {
        matches!(
            display.inside(),
            DisplayInside::Flow | DisplayInside::FlowRoot
        )
    });
    if !in_block_container {
        return false;
    }
    let children = nodes[parent].layout_children.borrow();
    let Some(children) = children.as_ref() else {
        return false;
    };
    children
        .iter()
        .take_while(|&&child| child != node.id)
        .any(|&child| {
            nodes[child].primary_styles().is_none_or(|computed| {
                !computed.clone_position().is_absolutely_positioned()
                    && computed.clone_float() == Float::None
            })
        })
}

/// The floats a `<br>` clears: its `clear`, or its `clear` attribute,
/// which Chrome maps to the property: `left`, `right`, and `all` or `both`.
fn br_clear(
    computed: &ComputedValues,
    element: &crate::node::ElementData,
) -> Option<winkin::Clear> {
    use ::style::values::computed::Clear as StyloClear;
    match computed.clone_clear() {
        StyloClear::Left | StyloClear::InlineStart if !is_rtl(computed) => {
            return Some(winkin::Clear::Left);
        }
        StyloClear::Right | StyloClear::InlineEnd if !is_rtl(computed) => {
            return Some(winkin::Clear::Right);
        }
        StyloClear::Left | StyloClear::InlineEnd => return Some(winkin::Clear::Left),
        StyloClear::Right | StyloClear::InlineStart => return Some(winkin::Clear::Right),
        StyloClear::Both => return Some(winkin::Clear::Both),
        StyloClear::None => {}
    }
    let clear = element.attr(local_name!("clear"))?.trim();
    if clear.eq_ignore_ascii_case("left") {
        Some(winkin::Clear::Left)
    } else if clear.eq_ignore_ascii_case("right") {
        Some(winkin::Clear::Right)
    } else if clear.eq_ignore_ascii_case("all") || clear.eq_ignore_ascii_case("both") {
        Some(winkin::Clear::Both)
    } else {
        None
    }
}

/// Whether `computed` is right-to-left.
fn is_rtl(computed: &ComputedValues) -> bool {
    computed.clone_direction() == ::style::computed_values::direction::T::Rtl
}

/// What takes a point of the area the lines were broken in -- along the lines
/// from their line-left end, across them from the block-start edge -- onto the
/// content box, which is `content_width` device pixels wide and
/// `content_height` tall.
///
/// The identity for horizontal text. A vertical block's lines run down the
/// box and stack across it, from the right in `vertical-rl` and from the left
/// in `vertical-lr`; `sideways-lr` runs its lines up the box, so its origin is
/// the bottom edge.
pub fn page(writing_mode: WritingMode, content_width: f64, content_height: f64) -> Affine {
    match writing_mode {
        WritingMode::VerticalRl | WritingMode::SidewaysRl => {
            Affine::new([0.0, 1.0, -1.0, 0.0, content_width, 0.0])
        }
        WritingMode::VerticalLr => Affine::new([0.0, 1.0, 1.0, 0.0, 0.0, 0.0]),
        WritingMode::SidewaysLr => Affine::new([0.0, -1.0, 1.0, 0.0, 0.0, content_height]),
        WritingMode::HorizontalTb => Affine::IDENTITY,
    }
}

/// What takes a point of a line -- along it from its line box's left, across
/// it from its over edge -- onto the content box.
///
/// `vertical-lr` stacks its lines from the left while each line's over side
/// is its right, so a line is mirrored across its own box there.
pub fn line_frame(writing_mode: WritingMode, page: Affine, metrics: &LineMetrics) -> Affine {
    let placed = Affine::translate((f64::from(metrics.left), f64::from(metrics.top)));
    if writing_mode == WritingMode::VerticalLr {
        let across = f64::from(2.0 * metrics.top + metrics.height());
        page * Affine::new([1.0, 0.0, 0.0, -1.0, 0.0, across]) * placed
    } else {
        page * placed
    }
}

/// Whether the body is right-to-left, where `element` is the root element,
/// has a body, and neither is contained; `None` otherwise.
fn body_is_rtl(nodes: &crate::NodeTree, element: NodeId) -> Option<bool> {
    let html = &nodes[element];
    let parent = html.parent?;
    if !matches!(nodes[parent].data, NodeData::Document(_))
        || html
            .data
            .downcast_element()
            .is_none_or(|data| data.name.local != local_name!("html"))
    {
        return None;
    }
    let body = html
        .children
        .iter()
        .map(|&child| &nodes[child])
        .find(|child| {
            child
                .data
                .downcast_element()
                .is_some_and(|data| data.name.local == local_name!("body"))
        })?;
    // Containment on either keeps the body's direction its own.
    let contained = |node: &Node| {
        node.primary_styles()
            .is_some_and(|computed| !computed.clone_contain().is_empty())
    };
    if contained(html) || contained(body) {
        return None;
    }
    let computed = body.primary_styles()?;
    Some(computed.clone_direction() == ::style::computed_values::direction::T::Rtl)
}

/// How far `position: relative` moves what `key` paints, in CSS pixels:
/// the offsets of its inline box and every inline box it is in, up to the
/// block the lines belong to, whose own offset moves its whole box.
///
/// `containing` is the block's content box in CSS pixels, which percentages
/// are of, and `rtl` its direction, under which `right` outweighs `left`.
pub fn relative_shift(
    doc: &BaseDocument,
    key: u64,
    containing: kurbo::Size,
    rtl: bool,
) -> kurbo::Vec2 {
    relative_shift_of(doc.get_node(NodeId::from_u64(key)), key, containing, rtl)
}

/// [`relative_shift`] from `key`'s node, where there is one.
pub(crate) fn relative_shift_of(
    node: Option<&Node>,
    key: u64,
    containing: kurbo::Size,
    rtl: bool,
) -> kurbo::Vec2 {
    use ::style::computed_values::position::T as Position;
    use ::style::values::computed::{CSSPixelLength, Inset};
    let mut shift = kurbo::Vec2::ZERO;
    if key & (FIRST_LETTER_KEY | MARKER_KEY) != 0 {
        return shift;
    }
    let resolve = |inset: &Inset, basis: f64| match inset {
        Inset::LengthPercentage(length) => Some(f64::from(
            length.resolve(CSSPixelLength::new(basis as f32)).px(),
        )),
        _ => None,
    };
    let mut at = node;
    while let Some(node) = at {
        match &node.data {
            NodeData::Text(_) => {}
            NodeData::Element(_) => {
                let Some(computed) = node.primary_styles() else {
                    break;
                };
                let display = computed.clone_display();
                if !display.is_contents() {
                    // An atomic inline or a block moves as a box of its own.
                    if (display.outside(), display.inside())
                        != (DisplayOutside::Inline, DisplayInside::Flow)
                    {
                        break;
                    }
                    if computed.clone_position() == Position::Relative {
                        let insets = computed.get_position();
                        let left = resolve(&insets.left, containing.width);
                        let right = resolve(&insets.right, containing.width);
                        shift.x += match (left, right) {
                            (Some(left), Some(_)) if !rtl => left,
                            (_, Some(right)) => -right,
                            (Some(left), None) => left,
                            (None, None) => 0.0,
                        };
                        let top = resolve(&insets.top, containing.height);
                        let bottom = resolve(&insets.bottom, containing.height);
                        shift.y += top.or(bottom.map(|bottom| -bottom)).unwrap_or(0.0);
                    }
                }
            }
            _ => break,
        }
        at = node.parent.map(|parent| node.with(parent));
    }
    shift
}

/// The parts of `key`'s inline box on each line, as rects in the content
/// box, in device pixels: one a line, the union of its parts there, as
/// `getClientRects` answers.
///
/// The content box is `content` device pixels in size, which the vertical
/// writing modes place their lines in from the right or the bottom.
pub(crate) fn box_rects(
    layout: &Layout,
    writing_mode: WritingMode,
    content: kurbo::Size,
    key: u64,
) -> impl Iterator<Item = kurbo::Rect> + '_ {
    let page = page(writing_mode, content.width, content.height);
    let mut fragments = layout.box_fragments(NodeKey(key)).peekable();
    core::iter::from_fn(move || {
        let first = fragments.next()?;
        let line = first.line();
        let mut united = fragment_rect(&first);
        while let Some(next) = fragments.next_if(|fragment| fragment.line() == line) {
            united = united.union(fragment_rect(&next));
        }
        let metrics = layout.line(line)?.metrics();
        Some(line_frame(writing_mode, page, &metrics).transform_rect_bbox(united))
    })
}

/// A box part's border box in its line's terms: along the line from its
/// line box's left, across it from its over edge.
fn fragment_rect(fragment: &winkin::BoxFragment<'_>) -> kurbo::Rect {
    let inline = fragment.inline();
    let block = fragment.block();
    kurbo::Rect::new(
        f64::from(inline.left),
        f64::from(block.over),
        f64::from(inline.right),
        f64::from(block.under),
    )
}

/// A piece of a line's content, as [`logical_content`] reads it.
#[derive(Clone)]
enum LinePiece {
    /// The text between these byte offsets of the layout text, of the node the key names, or of
    /// the node the layout maps its start to.
    Text(Option<u64>, core::ops::Range<usize>),
    /// An atomic inline, a float or an absolutely positioned box.
    Box(u64),
}

/// The laid-out text and inline boxes of `layout` in logical order, as its lines place them, read
/// as it is walked: each stretch of the layout text one node holds as one item. A float or an
/// absolutely positioned box follows the content of the line it is anchored on, and one anchored
/// on no line follows the last line.
pub(crate) fn logical_content(
    layout: &Layout,
) -> impl Iterator<Item = crate::node::InlineContent> + '_ {
    coalesce(content_pieces(layout))
}

/// Joins each run of text items of one node that follow on in the text into one item.
fn coalesce(
    mut items: impl Iterator<Item = crate::node::InlineContent>,
) -> impl Iterator<Item = crate::node::InlineContent> {
    use crate::node::InlineContent;
    let mut pending: Option<InlineContent> = None;
    core::iter::from_fn(move || {
        loop {
            let Some(item) = items.next() else {
                return pending.take();
            };
            if let (
                Some(InlineContent::Text { node_id, range }),
                InlineContent::Text {
                    node_id: next_id,
                    range: next,
                },
            ) = (&mut pending, &item)
                && node_id == next_id
                && range.end == next.start
            {
                range.end = next.end;
                continue;
            }
            if let Some(read) = pending.replace(item) {
                return Some(read);
            }
        }
    })
}

/// The pieces of `layout`'s content in logical order: each slice the text copies as, and each
/// box.
fn content_pieces(layout: &Layout) -> impl Iterator<Item = crate::node::InlineContent> + '_ {
    use crate::node::InlineContent;
    use winkin::Item;
    use winkin::selection::{Affinity, CopyKind, Position};
    let shown = |key: u64| key & (FIRST_LETTER_KEY | MARKER_KEY) == 0;
    let text = layout.text();
    let mut lines = layout.lines();
    let mut statics = layout.static_positions().peekable();
    let mut line_floats = 0;
    let mut past_lines = false;
    // The current line's items, sorted into logical order, and its pieces
    let mut items: Vec<(core::ops::Range<usize>, u64, bool)> = Vec::new();
    let mut pieces = Vec::new();
    let mut next_piece = 0;
    // The current text piece's node, its start and the slices it copies as
    let mut slices = None;
    core::iter::from_fn(move || {
        loop {
            if slices.is_none() {
                while next_piece == pieces.len() {
                    pieces.clear();
                    next_piece = 0;
                    if let Some(line) = lines.next() {
                        items.clear();
                        items.extend(line.all_items().filter_map(|item| match item {
                            Item::Text(run) => Some((run.text_range(), run.key().0, false)),
                            Item::Atomic(atomic) => {
                                Some((atomic.text_range(), atomic.key().0, true))
                            }
                            _ => None,
                        }));
                        items.sort_by_key(|(range, ..)| range.start);
                        // What the runs leave of the line, such as a forced break or a space the
                        // line wraps at, goes with the node the layout maps it to.
                        let line_range = line.text_range();
                        let mut cursor = line_range.start;
                        for (range, key, atomic) in items.drain(..) {
                            if range.start > cursor {
                                pieces.push(LinePiece::Text(None, cursor..range.start));
                            }
                            if !atomic {
                                pieces.push(LinePiece::Text(Some(key), range.clone()));
                            } else if shown(key) {
                                pieces.push(LinePiece::Box(key));
                            }
                            cursor = cursor.max(range.end);
                        }
                        if line_range.end > cursor {
                            pieces.push(LinePiece::Text(None, cursor..line_range.end));
                        }
                        for float in line.floats() {
                            line_floats += 1;
                            if shown(float.key.0) {
                                pieces.push(LinePiece::Box(float.key.0));
                            }
                        }
                        let index = line.index();
                        while let Some(position) = statics
                            .next_if(|position| position.line.is_some_and(|line| line <= index))
                        {
                            pieces.push(LinePiece::Box(position.key.0));
                        }
                    } else if !past_lines {
                        past_lines = true;
                        let floats = layout.floats().skip(line_floats);
                        pieces.extend(
                            floats
                                .map(|float| float.key.0)
                                .filter(|&key| shown(key))
                                .map(LinePiece::Box),
                        );
                        pieces.extend(
                            statics
                                .by_ref()
                                .map(|position| LinePiece::Box(position.key.0)),
                        );
                    } else {
                        return None;
                    }
                }
                let piece = pieces[next_piece].clone();
                next_piece += 1;
                match piece {
                    LinePiece::Box(key) => {
                        return Some(InlineContent::Box(NodeId::from_u64(key)));
                    }
                    LinePiece::Text(key, range) => {
                        let key = key.or_else(|| {
                            let position = Position::new(range.start, Affinity::Downstream);
                            layout.node_position(position).map(|node| node.key.0)
                        });
                        let Some(key) = key.filter(|&key| shown(key)) else {
                            continue;
                        };
                        let copied = layout.selected_text(range, CopyKind::Text);
                        slices = Some((NodeId::from_u64(key), copied));
                    }
                }
            }
            let (node_id, copied) = slices.as_mut()?;
            let Some(slice) = copied.next() else {
                slices = None;
                continue;
            };
            // The slices borrow from the layout text, which says where each is.
            let at = (slice.as_ptr() as usize).wrapping_sub(text.as_ptr() as usize);
            if slice.is_empty() || at >= text.len() {
                continue;
            }
            return Some(InlineContent::Text {
                node_id: *node_id,
                range: at..at + slice.len(),
            });
        }
    })
}

/// Hit tests physical CSS-pixel coordinates relative to the inline root's content box. `exact`
/// requires the point to be within a line and its text extent.
pub(crate) fn hit_test(
    text: &TextLayout,
    x: f32,
    y: f32,
    exact: bool,
) -> Option<crate::node::InlineTextHit> {
    let layout = text.layout()?;
    let scale = text.scale;
    let page = page(
        text.writing_mode(),
        text.content_size.width,
        text.content_size.height,
    );
    let point = page.inverse()
        * kurbo::Point::new(
            f64::from(x * scale),
            f64::from((y - text.block_offset) * scale),
        );
    let (inline, block) = (point.x as f32, point.y as f32);
    if exact
        && !layout.lines().any(|line| {
            let metrics = line.metrics();
            block >= metrics.top
                && block < metrics.top + metrics.height()
                && inline >= metrics.left
                && inline < metrics.left + metrics.width
        })
    {
        return None;
    }
    let position = layout.hit_test(inline, block, winkin::config::PastLines::Column)?;
    let node = layout.node_position(position)?;
    let node_id = NodeId::from_u64(node.key.0);
    Some(crate::node::InlineTextHit {
        node_id,
        byte_offset: position.offset,
    })
}

/// Calls `f` with each rectangle that highlights the text between `start` and `end`, in physical
/// device pixels relative to the content box.
pub(crate) fn for_each_selection_rect(
    text: &TextLayout,
    start: usize,
    end: usize,
    mut f: impl FnMut(kurbo::Rect),
) {
    let Some(layout) = text.layout() else {
        return;
    };
    let mode = text.writing_mode();
    let page = page(mode, text.content_size.width, text.content_size.height);
    for selection in layout.selection_rects(start..end) {
        let Some(line) = layout.line(selection.line) else {
            continue;
        };
        let rect = kurbo::Rect::new(
            f64::from(selection.inline.left),
            f64::from(selection.block.over),
            f64::from(selection.inline.right),
            f64::from(selection.block.under),
        );
        f(line_frame(mode, page, &line.metrics()).transform_rect_bbox(rect));
    }
}

/// The text between byte offsets `start` and `end` of the laid-out text, as it is copied, in the
/// pieces of the text it is made of.
pub(crate) fn selected_text(
    text: &TextLayout,
    start: usize,
    end: usize,
) -> impl Iterator<Item = &str> {
    text.layout()
        .filter(|layout| start < end && end <= layout.text().len())
        .into_iter()
        .flat_map(move |layout| layout.selected_text(start..end, winkin::selection::CopyKind::Text))
}

/// The parts of `node`'s inline box on each line, in CSS pixels relative to the border box of the
/// inline root `root`, whose content box starts at `origin_x` and `origin_y`, computed as they are
/// walked.
pub(crate) fn fragment_rects(
    layout: &Layout,
    writing_mode: WritingMode,
    root: &Node,
    node: &Node,
    origin_x: f32,
    origin_y: f32,
    scale: f32,
) -> impl Iterator<Item = taffy::Rect<f32>> {
    let root_layout = root.unrounded_layout();
    let scale = f64::from(scale);
    let content = kurbo::Size::new(
        f64::from(root_layout.content_box_width()) * scale,
        f64::from(root_layout.content_box_height()) * scale,
    );
    // `position: relative` moves the box and the boxes it is in.
    let shift = relative_shift_of(
        Some(node),
        node.id.as_u64(),
        content / scale,
        root.primary_styles().is_some_and(|styles| {
            styles.clone_direction() == ::style::computed_values::direction::T::Rtl
        }),
    );
    box_rects(layout, writing_mode, content, node.id.as_u64()).map(move |rect| taffy::Rect {
        left: origin_x + (shift.x + rect.x0 / scale) as f32,
        top: origin_y + (shift.y + rect.y0 / scale) as f32,
        right: origin_x + (shift.x + rect.x1 / scale) as f32,
        bottom: origin_y + (shift.y + rect.y1 / scale) as f32,
    })
}

impl InlineText for TextLayout {
    #[inline]
    fn scale(&self) -> f32 {
        self.scale
    }

    #[inline]
    fn block_offset(&self) -> f32 {
        self.block_offset
    }

    #[inline]
    fn text(&self) -> &str {
        self.layout().map_or("", |layout| layout.text())
    }

    #[inline]
    fn text_len(&self) -> usize {
        self.text().len()
    }

    fn selected_text(&self, start: usize, end: usize) -> impl Iterator<Item = &str> {
        selected_text(self, start, end)
    }

    fn logical_content(&self) -> impl Iterator<Item = InlineContent> {
        self.layout().into_iter().flat_map(logical_content)
    }

    fn source_offset(&self, node_id: NodeId, offset: usize) -> Option<usize> {
        let layout = self.layout()?;
        let position = layout.position(
            NodeKey(node_id.as_u64()),
            offset,
            winkin::selection::Affinity::Downstream,
        )?;
        Some(position.offset)
    }

    #[inline]
    fn maps_source(&self) -> bool {
        true
    }

    fn first_baseline(&self) -> Option<f32> {
        let line = self.layout()?.lines().next()?;
        Some(line.metrics().baseline / self.scale + self.block_offset)
    }

    fn hit_test(&self, x: f32, y: f32, exact: bool) -> Option<InlineTextHit> {
        hit_test(self, x, y, exact)
    }

    fn for_each_selection_rect(&self, start: usize, end: usize, f: impl FnMut(kurbo::Rect)) {
        for_each_selection_rect(self, start, end, f);
    }

    fn fragment_rects(&self, root: &Node, node: &Node) -> impl Iterator<Item = taffy::Rect<f32>> {
        let root_layout = root.unrounded_layout();
        let content_box_inset = root_layout.padding + root_layout.border;
        self.layout().into_iter().flat_map(move |layout| {
            fragment_rects(
                layout,
                self.writing_mode,
                root,
                node,
                content_box_inset.left,
                content_box_inset.top + self.block_offset,
                self.scale,
            )
        })
    }

    fn debug_print(&self) {
        let Some(layout) = self.layout() else {
            println!("Not laid out");
            return;
        };
        println!("Text content: {:?}", layout.text());
        println!("Lines:");
        for line in layout.lines() {
            let metrics = line.metrics();
            println!(
                "Line {}: left:{} top:{} width:{} height:{} text:{:?}",
                line.index(),
                metrics.left,
                metrics.top,
                metrics.width,
                metrics.height(),
                line.text_range(),
            );
        }
    }
}

impl InlineLayoutEngine for TextLayout {
    const SETS_WRITING_MODES: bool = true;
    const READS_ROOM_ABOVE: bool = true;

    fn build_layouts(
        _cx: &mut TextContext,
        _nodes: &crate::NodeTree,
        _scale: f32,
        layouts: &mut [(NodeId, Box<Self>)],
    ) {
        // winkin builds the content when the context is next laid out, once
        // its atomic inlines are measured.
        for (_, layout) in layouts {
            layout.invalidate();
        }
    }

    fn is_empty(&self) -> bool {
        // The content is built when it is laid out.
        false
    }

    fn prepare(&mut self, doc: &mut BaseDocument, root: NodeId, lines: LinesInputs<'_>) {
        let scale = doc.viewport.scale();
        // Each border box, and where an atomic inline's baseline is, in device
        // pixels. In a vertical line a box's height is along it, and it sits on
        // no baseline of its own.
        let sizes: Vec<BuiltBox> = lines
            .sizes
            .iter()
            .map(|measured| BuiltBox {
                node: measured.node.as_u64(),
                size: if lines.vertical {
                    BoxSize {
                        inline: measured.size.height * scale,
                        block: measured.size.width * scale,
                        baseline: None,
                    }
                } else {
                    BoxSize {
                        inline: measured.size.width * scale,
                        block: measured.size.height * scale,
                        baseline: measured.baseline.map(|baseline| baseline * scale),
                    }
                },
            })
            .collect();
        if self.is_built_with(&sizes, lines.basis) {
            return;
        }
        let guard = doc.guard.read();
        build(
            &doc.nodes,
            Cascade {
                stylist: &doc.stylist,
                guards: &StylesheetGuards::same(&guard),
            },
            &mut doc.text.cx,
            self,
            scale,
            lines.basis,
            root,
            &sizes,
        );
    }

    fn content_widths(&mut self) -> ContentWidths {
        let widths = self.intrinsic_widths();
        ContentWidths {
            min: widths.min_content,
            max: widths.max_content,
        }
    }

    fn break_lines(
        &mut self,
        cx: &mut TextContext,
        area: LineArea,
        _style: Option<&ComputedValues>,
        exclusions: &mut impl LineExclusions,
    ) {
        let area = Area {
            room_above: area.room_above,
            block_end: area.block_end,
            ..Area::new(area.width)
        };
        self.lay_out(&mut cx.cx, area, &mut ExclusionsOf(exclusions));
    }

    fn extent(&self, end_padding: f32) -> LinesExtent {
        let Some(layout) = self.layout() else {
            return LinesExtent::default();
        };
        let metrics = layout.metrics();
        LinesExtent {
            height: metrics.block_end - into_end_padding(layout.room_below(), end_padding),
            width: layout
                .lines()
                .map(|line| {
                    let metrics = line.metrics();
                    metrics.left + metrics.width
                })
                .fold(0.0, f32::max),
            first_baseline: metrics.first_baseline,
            last_baseline: metrics.last_baseline,
        }
    }

    fn set_frame(&mut self, block_offset: f32, content: taffy::Size<f32>) {
        self.block_offset = block_offset;
        self.content_size = kurbo::Size::new(f64::from(content.width), f64::from(content.height));
    }

    fn placements(&self) -> impl Iterator<Item = Placement> {
        let mut placed = Vec::new();
        if let Some(layout) = self.layout() {
            if self.is_vertical() {
                self.vertical_placements(layout, &mut placed);
            } else {
                horizontal_placements(layout, &mut placed);
            }
        }
        placed.into_iter()
    }

    fn line_flow(&self) -> LineFlow {
        match self.writing_mode {
            WritingMode::HorizontalTb => LineFlow::Horizontal,
            WritingMode::VerticalRl => LineFlow::VerticalRl,
            WritingMode::VerticalLr => LineFlow::VerticalLr,
            WritingMode::SidewaysRl => LineFlow::SidewaysRl,
            WritingMode::SidewaysLr => LineFlow::SidewaysLr,
        }
    }

    fn place_on_page(&self, along: [f32; 2], across: [f32; 2]) -> (f32, f32) {
        let content = self.content_size;
        let (width, height) = (content.width as f32, content.height as f32);
        let [left, right] = along;
        let [start, end] = across;
        match self.writing_mode {
            WritingMode::VerticalRl | WritingMode::SidewaysRl => (width - end, left),
            WritingMode::VerticalLr => (start, left),
            WritingMode::SidewaysLr => (start, height - right),
            WritingMode::HorizontalTb => (left, start),
        }
    }

    fn last_line_baseline(&self) -> LastBaseline {
        let Some(layout) = self.layout() else {
            return LastBaseline::Unknown;
        };
        if self.is_vertical() {
            return LastBaseline::Unknown;
        }
        match layout.metrics().last_baseline {
            Some(baseline) => LastBaseline::At(baseline),
            None => LastBaseline::None,
        }
    }

    fn room_below(&self) -> f32 {
        self.layout().map_or(0.0, |layout| layout.room_below())
    }

    fn float_node(key: u64) -> Option<NodeId> {
        // The block's initial letter and its inside marker have no node.
        (key & (FIRST_LETTER_KEY | MARKER_KEY) == 0).then(|| NodeId::from_u64(key))
    }

    fn inline_shift(
        doc: &BaseDocument,
        node: NodeId,
        containing: taffy::Size<f32>,
        rtl: bool,
    ) -> taffy::Point<f32> {
        let shift =
            doc.get_node(node)
                .and_then(|node| node.parent)
                .map_or(kurbo::Vec2::ZERO, |parent| {
                    relative_shift(
                        doc,
                        parent.as_u64(),
                        kurbo::Size::new(f64::from(containing.width), f64::from(containing.height)),
                        rtl,
                    )
                });
        taffy::Point {
            x: shift.x as f32,
            y: shift.y as f32,
        }
    }
}

impl TextLayout {
    /// What the lines of a vertical layout placed, turned onto the page: each
    /// atomic inline's margin box, and where each absolutely positioned box
    /// would have been.
    fn vertical_placements(&self, layout: &Layout, placed: &mut Vec<Placement>) {
        let writing_mode = self.writing_mode;
        let at = |node: u64, (x, y): (f32, f32), rtl| Placement {
            node: NodeId::from_u64(node),
            x,
            top: y,
            line_top: y,
            line_bottom: y,
            block_start: y,
            rtl,
        };
        for line in layout.lines() {
            let metrics = line.metrics();
            // `vertical-lr` stacks its lines from the left while each line's
            // over side is its right.
            let block = |over: f32, under: f32| {
                if writing_mode == WritingMode::VerticalLr {
                    let bottom = metrics.top + metrics.height();
                    [bottom - under, bottom - over]
                } else {
                    [metrics.top + over, metrics.top + under]
                }
            };
            for item in line.items() {
                if let Item::Atomic(atomic) = item {
                    let inline = atomic.inline();
                    let cross = atomic.block();
                    let page = self.place_on_page(
                        [metrics.left + inline.left, metrics.left + inline.right],
                        block(cross.over, cross.under),
                    );
                    placed.push(at(atomic.key().0, page, None));
                }
            }
        }
        // An absolutely positioned box's block-start edge, across the page,
        // faces right where the lines stack from the right.
        let from_right = matches!(
            writing_mode,
            WritingMode::VerticalRl | WritingMode::SidewaysRl
        );
        for position in layout.static_positions() {
            let page = self.place_on_page(
                [position.inline, position.inline],
                [position.block, position.block],
            );
            placed.push(at(position.key.0, page, Some(from_right)));
        }
    }
}

/// What the lines of a horizontal layout placed: each atomic inline's margin
/// box on its line, and where each absolutely positioned box would have been,
/// as winkin finds its static position: its line box spans the block axis of
/// an inline-level box's static-position rectangle.
fn horizontal_placements(layout: &Layout, placed: &mut Vec<Placement>) {
    for line in layout.lines() {
        let metrics = line.metrics();
        for item in line.items() {
            if let Item::Atomic(atomic) = item {
                let inline = atomic.inline();
                let block = atomic.block();
                placed.push(Placement {
                    node: NodeId::from_u64(atomic.key().0),
                    x: metrics.left + inline.left,
                    top: metrics.top + block.over,
                    line_top: metrics.top,
                    line_bottom: metrics.top + metrics.height(),
                    block_start: metrics.top,
                    rtl: None,
                });
            }
        }
    }
    for at in layout.static_positions() {
        let bottom = at
            .line
            .and_then(|line| layout.line(line))
            .map_or(at.block, |line| {
                let metrics = line.metrics();
                (metrics.top + metrics.height()).max(at.block)
            });
        placed.push(Placement {
            node: NodeId::from_u64(at.key.0),
            x: at.inline,
            top: at.block,
            line_top: at.block,
            line_bottom: bottom,
            block_start: at.block,
            rtl: Some(at.direction == winkin::style::Direction::Rtl),
        });
    }
}

/// How far the last line's ruby annotations and emphasis marks reach into
/// the block's end padding, `padding` device pixels, where they reach past
/// its line box: the layout's end counts them, and Chrome lets them into
/// the padding rather than make the block taller.
fn into_end_padding(room_below: f32, padding: f32) -> f32 {
    (-room_below).clamp(0.0, padding.max(0.0))
}

/// The floats Blitz keeps an inline formatting context's lines clear of, as
/// winkin asks after them.
struct ExclusionsOf<'a, E>(&'a mut E);

impl<E: LineExclusions> Exclusions for ExclusionsOf<'_, E> {
    fn band(&self, _line: usize, block: BlockExtents) -> InlineExtents {
        let (left, right) = self.0.band(block.start, block.end);
        InlineExtents { left, right }
    }

    fn below(&self, top: f32) -> Option<f32> {
        self.0.below(top)
    }

    fn place(&mut self, float: winkin::FloatRequest) -> PlacedFloat {
        let placed = self.0.place(FloatRequest {
            key: float.key.0,
            side: match float.side {
                FloatSide::Left => super::FloatSide::Left,
                FloatSide::Right => super::FloatSide::Right,
            },
            inline_size: float.inline_size,
            block_size: float.block_size,
            block_start: float.block_start,
        });
        PlacedFloat {
            inline: InlineExtents {
                left: placed.left,
                right: placed.right,
            },
            block: BlockExtents {
                start: placed.top,
                end: placed.bottom,
            },
        }
    }

    fn checkpoint(&self) -> ExclusionsCheckpoint {
        ExclusionsCheckpoint(self.0.checkpoint() as u64)
    }

    fn rewind(&mut self, to: ExclusionsCheckpoint) {
        self.0.rewind(to.0 as usize);
    }
}

/// Builds `content`, the value of the `<input>` or `<textarea>` `node`, into `text`: set in
/// `computed`, its computed style, with white space preserved (as `break-spaces` where the style
/// asks for it) and no text transform, so that offsets in the layout's text are offsets in
/// `content`. The lines wrap at `width` device pixels as the style wraps them, or not at all
/// where it is `None`.
pub(crate) fn build_plain_text(
    cx: &mut TextContext,
    text: &mut TextLayout,
    node: NodeId,
    computed: &ComputedValues,
    content: &str,
    scale: f32,
    width: Option<f32>,
) {
    let feature_values = style::FeatureValues::default();
    let lists = style::FontLists::of(computed, &feature_values);
    let own = style::computed_style(&lists, computed, scale, 0.0, None);
    let own = ComputedStyle {
        text: winkin::style::TextGroup {
            white_space_collapse: match own.text.white_space_collapse {
                winkin::style::WhiteSpaceCollapse::BreakSpaces => {
                    winkin::style::WhiteSpaceCollapse::BreakSpaces
                }
                _ => winkin::style::WhiteSpaceCollapse::Preserve,
            },
            wrap_mode: match width {
                Some(_) => own.text.wrap_mode,
                None => winkin::style::TextWrapMode::NoWrap,
            },
            transform: winkin::style::TextGroup::INITIAL.transform,
            ..own.text
        },
        ..own
    };
    let block = style::block_style(&own, None, computed, computed, scale);
    let key = NodeKey(node.as_u64());
    text.built = false;
    text.laid = false;
    text.boxes.clear();
    text.basis = 0.0;
    text.scale = scale;
    text.writing_mode = block.writing_mode;
    text.styles.first_line.clear();
    text.styles.first_letter = None;
    text.styles.marker = None;
    text.styles.marks.clear();
    let mut builder = text.layout.builder(key, &block, BuildOptions::default());
    builder.text(key, content);
    builder.finish(&mut cx.cx);
    text.built = true;
    let area = Area::new(width.unwrap_or(f32::MAX / 4.0));
    text.lay_out(&mut cx.cx, area, &mut winkin::NoExclusions);
}

impl MarkerEngine for TextLayout {
    fn build(
        cx: &mut TextContext,
        node: NodeId,
        computed: &ComputedValues,
        marker: &Marker,
        bullet: bool,
        scale: f32,
    ) -> Self {
        let mut text = TextLayout {
            scale,
            ..TextLayout::default()
        };
        let key = NodeKey(MARKER_KEY | node.as_u64());
        text.styles.marker = Some(ServoArc::new(computed.clone()));
        let feature_values = style::FeatureValues::default();
        let mut lists = style::FontLists::of(computed, &feature_values);
        if bullet {
            lists = lists.with_families_first(&BULLET_FAMILIES);
        }
        let own = unboxed(style::computed_style(&lists, computed, scale, 0.0, None));
        let block = ComputedBlockStyle {
            writing_mode: WritingMode::HorizontalTb,
            ..ComputedBlockStyle::new(&own)
        };
        let mut builder = text.layout.builder(key, &block, BuildOptions::default());
        match marker {
            Marker::Char(char) => builder.text(key, char.encode_utf8(&mut [0; 4])),
            Marker::String(string) => builder.text(key, string),
        }
        builder.finish(&mut cx.cx);
        text.built = true;
        text.lay_out(
            &mut cx.cx,
            Area::new(f32::MAX / 4.0),
            &mut winkin::NoExclusions,
        );
        text
    }
}
