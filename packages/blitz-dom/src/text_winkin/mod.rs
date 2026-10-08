//! Inline formatting contexts laid out by winkin rather than Parley.
//!
//! The content of a context is built from the DOM the first time it is laid
//! out, and again whenever it changes or its atomic inlines measure
//! differently: winkin takes each atomic inline's size as it is pushed, and
//! Blitz only knows it once taffy has measured the box.

pub(crate) mod style;

use ::style::context::{CascadeInputs, TreeCountingCaches};
use ::style::properties::{ComputedValues, FirstLineReparenting};
use ::style::rule_cache::RuleCacheConditions;
use ::style::selector_parser::PseudoElement;
use ::style::servo_arc::Arc as ServoArc;
use ::style::shared_lock::StylesheetGuards;
use ::style::stylist::Stylist;
use ::style::values::computed::{Display, Float};
use ::style::values::specified::box_::{DisplayInside, DisplayOutside};
use ::style::values::specified::position::PositionTryFallbacksTryTactic;
use blitz_traits::node_id::NodeId;
use fontwich::{Collection, FaceDescriptors, FontBytes, LayerBuilder, Role};
use kurbo::Affine;
use markup5ever::{local_name, ns};
use winkin::style::{
    BaseDirection, BidiGroup, EdgesGroup, FontFamilyName, GenericFamily, Language, LineGroup,
    UnicodeBidi, VerticalAlign, WritingMode,
};
use winkin::{
    Area, BoxSize, BuildOptions, ComputedBlockStyle, ComputedStyle, Context, Exclusions, FloatSide,
    IntrinsicSizes, Layout, LayoutBuilder, LineMetrics, NodeKey, OriginalDisplay,
};

use crate::layout::replaced::is_replaced_element;
use crate::node::{ListItemLayout, ListItemLayoutPosition, Marker};
use crate::{BaseDocument, Node, NodeData};

/// The key of a block's `::first-letter` box, which has no node of its own:
/// the block's node with this bit set.
pub const FIRST_LETTER_KEY: u64 = 1 << 62;

/// The key of a block's inside list marker, which has no node of its own:
/// the block's node with this bit set.
pub const MARKER_KEY: u64 = 1 << 61;

/// The fonts winkin chooses from, and the context it builds and breaks in.
///
/// The platform's fonts and the fonts Blitz ships are the collection's
/// lower layers; a document's `@font-face` fonts are a layer of their own
/// above them, which a face joins as it loads, the context being handed
/// the new collection.
pub(crate) struct WinkinFonts {
    /// What every layout is built and broken with.
    pub(crate) cx: Context,
    /// The fonts the document was handed, which its iframes are handed too.
    pub(crate) given: Collection,
    /// The installed and shipped fonts.
    base: Collection,
    /// The document's `@font-face` faces.
    document: LayerBuilder,
}

/// The platform's fonts where Blitz is built to read them.
///
/// Listing them reads every installed font file. Build one collection and
/// hand clones of it to each document through
/// [`DocumentConfig::winkin_fonts`](crate::DocumentConfig::winkin_fonts).
pub fn system_fonts() -> Collection {
    #[cfg(feature = "system-fonts")]
    return Collection::system();
    #[cfg(not(feature = "system-fonts"))]
    return Collection::new();
}

impl WinkinFonts {
    /// The `given` fonts, and the bullet font list markers are set in.
    pub(crate) fn new(given: Collection) -> Self {
        let mut shipped = LayerBuilder::new(Role::Application);
        let _ = shipped.add_data(FontBytes::from(crate::BULLET_FONT));
        let base = given.clone().with_layer(shipped.snapshot());
        let document = LayerBuilder::new(Role::Document);
        let cx = Context::new(base.clone().with_layer(document.snapshot()));
        Self {
            cx,
            given,
            base,
            document,
        }
    }

    /// Adds a web font that has loaded, under its `@font-face` rule's
    /// descriptors, and hands the context the collection with it.
    ///
    /// A rule with no family, or bytes that are not a font, adds nothing, as
    /// CSS falls back past a face whose download is no font.
    pub(crate) fn add_face(
        &mut self,
        bytes: impl AsRef<[u8]> + Send + Sync + 'static,
        family: Option<&str>,
        descriptors: FaceDescriptors,
    ) {
        let bytes = FontBytes::new(bytes);
        let added = match family {
            Some(family) => self
                .document
                .add_face(family, descriptors, Some((bytes, 0)))
                .is_ok(),
            None => self.document.add_data(bytes).is_ok(),
        };
        if added {
            self.cx
                .set_collection(self.base.clone().with_layer(self.document.snapshot()));
        }
    }
}

/// What winkin holds for one inline formatting context.
#[derive(Default)]
pub struct WinkinText {
    /// The content, and its lines once they are broken. Its allocations are
    /// reused by the next build.
    layout: Layout,
    /// Whether the layout holds content built for the node as it is now.
    built: bool,
    /// Whether the content has been broken into lines since it was built.
    laid: bool,
    /// What the atomic inlines and floats were built at, in content order,
    /// and what percentages of the containing block were resolved against.
    boxes: Vec<BoxMeasure>,
    basis: f32,
    /// The computed styles painted with that no node of the content's is
    /// styled in.
    styles: PaintStyles,
    /// Which way the content's lines run.
    writing_mode: WritingMode,
    /// Device pixels per CSS pixel used to build the layout.
    scale: f32,
}

/// A copy starts empty, and is built again the first time it is laid out.
impl Clone for WinkinText {
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

/// An atomic inline or a float as taffy measured it.
#[derive(Copy, Clone, PartialEq, Debug)]
pub(crate) struct BoxMeasure {
    pub(crate) node: u64,
    /// Its border box in device pixels, and its baseline where it has one.
    pub(crate) size: BoxSize,
}

impl WinkinText {
    pub fn scale(&self) -> f32 {
        self.scale
    }
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
    pub(crate) fn is_built_with(&self, sizes: &[BoxMeasure], basis: f32) -> bool {
        self.built && self.boxes == sizes && self.basis == basis
    }

    /// Returns the content's min-content and max-content widths.
    pub(crate) fn content_widths(&self) -> IntrinsicSizes {
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
    text: &mut WinkinText,
    scale: f32,
    basis: f32,
    root_id: NodeId,
    sizes: &[BoxMeasure],
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

    let WinkinText { layout, styles, .. } = text;
    let mut options = BuildOptions::default();
    // Hit testing needs to map a layout position back to its DOM node.
    options.map_source = true;
    let mut builder = layout.builder(key, &block, options);

    // A marker inside the list item comes before its content, in the bullet
    // font where it is a bullet.
    if let Some(ListItemLayout {
        marker,
        position: ListItemLayoutPosition::Inside,
    }) = root
        .element_data()
        .and_then(|element| element.list_item_data.as_deref())
    {
        let marker_key = NodeKey(MARKER_KEY | root_id.as_u64());
        styles.marker = Some(root_computed.clone());
        match marker {
            Marker::Char(char) => {
                let lists = style::FontLists::of(&root_computed, &feature_values)
                    .with_families_first(&BULLET_FAMILIES);
                let bullet = unboxed(style::computed_style(
                    &lists,
                    &root_computed,
                    scale,
                    basis,
                    language,
                ));
                builder.open_box(marker_key, &bullet, None);
                builder.text(marker_key, &format!("{char} "));
                builder.close_box();
            }
            Marker::String(string) => builder.text(marker_key, string),
        }
    }

    let mut walk = Walk {
        nodes,
        cascade,
        root,
        boxes: sizes,
        styles,
        first_letter: first_letter_computed,
        feature_values,
        marked: Vec::new(),
        scale,
        basis,
    };
    let parent = Parent {
        computed: &root_computed,
        first_line: first_line_computed.as_ref(),
        language,
        is_root: true,
    };
    for child_id in children_and_pseudos(root) {
        walk.node(&mut builder, parent, child_id);
    }
    let Walk {
        marked,
        feature_values,
        ..
    } = walk;
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

/// A node's `::before` pseudo-element, its children, and its `::after`, in
/// tree order.
fn children_and_pseudos(node: &Node) -> impl Iterator<Item = NodeId> + '_ {
    node.before()
        .into_iter()
        .chain(node.children.iter().copied())
        .chain(node.after())
}

/// What a node's children are set in: the box around them.
#[derive(Copy, Clone)]
struct Parent<'a> {
    computed: &'a ServoArc<ComputedValues>,
    /// Its style on the first formatted line, where the block has a
    /// `::first-line`.
    first_line: Option<&'a ServoArc<ComputedValues>>,
    language: Option<Language>,
    /// Whether it is the block itself.
    is_root: bool,
}

/// Walking the DOM into a builder.
struct Walk<'a> {
    nodes: &'a crate::NodeTree,
    cascade: Cascade<'a>,
    /// The block the content is for.
    root: &'a Node,
    boxes: &'a [BoxMeasure],
    styles: &'a mut PaintStyles,
    /// The block's `::first-letter` style, until its letter is found.
    first_letter: Option<ServoArc<ComputedValues>>,
    /// The document's `@font-feature-values` rules.
    feature_values: style::FeatureValues,
    /// The elements whose text takes emphasis marks, in the styles and
    /// languages their marks are set in.
    marked: Vec<(u64, ServoArc<ComputedValues>, Option<Language>)>,
    scale: f32,
    basis: f32,
}

impl Walk<'_> {
    /// Whether `node_id` holds an `<rt>`, which decides what an annotation
    /// container is: the annotation itself, or a wrapper around several.
    fn holds_annotation(&self, node_id: NodeId) -> bool {
        self.nodes[node_id].children.iter().any(|child| {
            matches!(
                &self.nodes[*child].data,
                NodeData::Element(element) if element.name.local == local_name!("rt")
            )
        })
    }

    /// Whether `node` is an `<rt>` inside an `<rtc>`, its annotation
    /// container.
    fn in_annotation_container(&self, node: &Node) -> bool {
        node.parent.is_some_and(|parent| {
            matches!(
                &self.nodes[parent].data,
                NodeData::Element(element) if element.name.local == local_name!("rtc")
            )
        })
    }

    /// The size taffy measured `node` at.
    fn measured(&self, node: u64) -> BoxSize {
        self.boxes
            .iter()
            .find(|size| size.node == node)
            .map_or(BoxSize::default(), |size| size.size)
    }

    /// Pushes `node_id`, a child of `parent` in the inline formatting
    /// context.
    fn node(&mut self, builder: &mut LayoutBuilder<'_>, parent: Parent<'_>, node_id: NodeId) {
        let node = &self.nodes[node_id];
        match &node.data {
            NodeData::Element(element) | NodeData::AnonymousBlock(element) => {
                if *element.name.local == *"input"
                    && element.attr(local_name!("type")) == Some("hidden")
                {
                    return;
                }
                let Some(computed) = node.primary_styles().map(|style| (*style).clone()) else {
                    return;
                };
                let display = node.display_style().unwrap_or(Display::inline());
                if matches!(
                    (display.outside(), display.inside()),
                    (DisplayOutside::None, DisplayInside::None)
                ) {
                    return;
                }
                let language = own_language(node).unwrap_or(parent.language);
                // Out of the flow: the content holds its anchor, which takes
                // no room, and the lines say where its static position is.
                if computed.clone_position().is_absolutely_positioned() {
                    let display = if computed.get_box().original_display.outside()
                        == DisplayOutside::Inline
                    {
                        OriginalDisplay::Inline
                    } else {
                        OriginalDisplay::Block
                    };
                    builder.absolute(NodeKey(node_id.as_u64()), display);
                    return;
                }
                match (display.outside(), display.inside()) {
                    (DisplayOutside::None, DisplayInside::Contents) => {
                        // No box, but its children inherit from it: a box
                        // with no edges and nothing painted, in its style.
                        self.open_box(
                            builder,
                            parent,
                            node,
                            &computed,
                            language,
                            BoxKind::Contents,
                        );
                    }
                    (DisplayOutside::Inline, DisplayInside::Flow) => {
                        let tag = &element.name.local;
                        if is_replaced_element(tag)
                            || *tag == local_name!("input")
                            || *tag == local_name!("textarea")
                            || *tag == local_name!("button")
                        {
                            self.atomic(builder, node_id, &computed, language);
                        } else if *tag == local_name!("br") {
                            self.first_letter = None;
                            let key = NodeKey(node_id.as_u64());
                            match br_clear(&computed, element) {
                                Some(clear) => builder.line_break_clearing(key, clear),
                                None => builder.line_break(key),
                            }
                        } else if *tag == local_name!("wbr") {
                            builder.break_opportunity();
                        } else if *tag == local_name!("rp") {
                            // The fallback parentheses, for a renderer that
                            // cannot set ruby. This one can.
                        } else {
                            // Ruby is known by its tags: Stylo's servo build
                            // has no `display: ruby` values, so the elements
                            // arrive as inline. The container bounds the
                            // bases and each `<rt>` is the annotation of the
                            // base before it. The parser has closed any
                            // `<rt>` the document left open.
                            let kind = if *tag == local_name!("ruby") {
                                self.first_letter = None;
                                BoxKind::Ruby
                            } else if *tag == local_name!("rt") {
                                BoxKind::Annotation
                            } else if *tag == local_name!("rtc") {
                                // An annotation container is the annotation
                                // itself where it holds no `<rt>`: the text
                                // inside it annotates the base before it. One
                                // that does hold `<rt>`s is transparent, each
                                // of them an annotation of its own.
                                if self.holds_annotation(node_id) {
                                    BoxKind::Contents
                                } else {
                                    BoxKind::Annotation
                                }
                            } else {
                                BoxKind::Span
                            };
                            self.open_box(builder, parent, node, &computed, language, kind);
                        }
                    }
                    _ => match computed.clone_float() {
                        Float::None => self.atomic(builder, node_id, &computed, language),
                        _ => self.float(builder, node_id, &computed, language),
                    },
                }
            }
            // A text node carries no style of its own: the box it is in is
            // what sets it, and a painter reads the element it is in.
            NodeData::Text(data) => {
                if data.content.is_empty() {
                    return;
                }
                self.arm_first_letter(builder, parent, &data.content);
                if let Some(element) = node.parent
                    && style::emphasis_mark_string(parent.computed).is_some()
                    && !self
                        .marked
                        .iter()
                        .any(|(held, ..)| *held == element.as_u64())
                {
                    self.marked
                        .push((element.as_u64(), parent.computed.clone(), parent.language));
                }
                builder.text(NodeKey(node_id.as_u64()), &data.content);
            }
            _ => {}
        }
    }

    /// Opens an inline box -- a span, a ruby container, an annotation, or
    /// the box a `display: contents` element's children inherit from -- and
    /// pushes its children into it.
    fn open_box(
        &mut self,
        builder: &mut LayoutBuilder<'_>,
        parent: Parent<'_>,
        node: &Node,
        computed: &ServoArc<ComputedValues>,
        language: Option<Language>,
        kind: BoxKind,
    ) {
        let node_id = node.id;
        let key = NodeKey(node_id.as_u64());
        // On the first line, the same element inheriting from its parent's
        // `::first-line` instead.
        let first_line = parent
            .first_line
            .map(|first_line| self.cascade.reparent(node, computed, first_line));
        if let Some(style) = &first_line {
            self.styles
                .first_line
                .push((node_id.as_u64(), style.clone()));
        }
        {
            let lists = style::FontLists::of(computed, &self.feature_values);
            let own = style::computed_style(&lists, computed, self.scale, self.basis, language);
            let first_line_lists = first_line
                .as_deref()
                .map(|computed| style::FontLists::of(computed, &self.feature_values));
            let first_line_style = match (&first_line, &first_line_lists) {
                (Some(computed), Some(lists)) => Some(style::computed_style(
                    lists, computed, self.scale, self.basis, language,
                )),
                _ => None,
            };
            match kind {
                BoxKind::Span => builder.open_box(key, &own, first_line_style.as_ref()),
                BoxKind::Ruby => builder.open_ruby(key, &own, first_line_style.as_ref()),
                // An `<rt>` in an `<rtc>` takes the side of its container,
                // which `ruby-position` applies to.
                BoxKind::Annotation if self.in_annotation_container(node) => builder
                    .open_annotation_with_position(
                        key,
                        &own,
                        first_line_style.as_ref(),
                        style::ruby_position(parent.computed),
                    ),
                BoxKind::Annotation => {
                    builder.open_annotation(key, &own, first_line_style.as_ref())
                }
                BoxKind::Contents => {
                    let first_line_style = first_line_style.map(unboxed);
                    builder.open_box(key, &unboxed(own), first_line_style.as_ref());
                }
            }
        }
        let this = Parent {
            computed,
            first_line: first_line.as_ref(),
            language,
            is_root: false,
        };
        for child_id in children_and_pseudos(node) {
            self.node(builder, this, child_id);
        }
        match kind {
            BoxKind::Span | BoxKind::Contents => builder.close_box(),
            BoxKind::Ruby => builder.close_ruby(),
            BoxKind::Annotation => builder.close_annotation(),
        }
    }

    /// Pushes an atomic inline, sized as taffy measured it.
    ///
    /// Its style says where it sits on the line and its margins; its border
    /// and padding are inside the border box taffy measured, and taffy paints
    /// it.
    fn atomic(
        &mut self,
        builder: &mut LayoutBuilder<'_>,
        node_id: NodeId,
        computed: &ComputedValues,
        language: Option<Language>,
    ) {
        // A block has one first letter, and it comes before any atomic inline.
        self.first_letter = None;
        let lists = style::FontLists::of(computed, &self.feature_values);
        let own = style::computed_style(&lists, computed, self.scale, self.basis, language);
        let own = ComputedStyle {
            edges: EdgesGroup {
                border: EdgesGroup::INITIAL.border,
                padding: EdgesGroup::INITIAL.padding,
                ..own.edges
            },
            paints: false,
            ..own
        };
        let size = self.measured(node_id.as_u64());
        builder.atomic(NodeKey(node_id.as_u64()), &own, None, size);
    }

    /// Pushes a float, which the block formatting context places when the
    /// text reaches it, the lines flowing around it.
    fn float(
        &mut self,
        builder: &mut LayoutBuilder<'_>,
        node_id: NodeId,
        computed: &ComputedValues,
        language: Option<Language>,
    ) {
        // `inline-start` and `inline-end` are the containing block's
        // sides, as its direction has them.
        let rtl = self.root.primary_styles().is_some_and(|block| {
            block.get_inherited_box().direction == ::style::computed_values::direction::T::Rtl
        });
        let side = match (computed.clone_float(), rtl) {
            (Float::Right, _) | (Float::InlineStart, true) | (Float::InlineEnd, false) => {
                FloatSide::Right
            }
            _ => FloatSide::Left,
        };
        let lists = style::FontLists::of(computed, &self.feature_values);
        let own = style::computed_style(&lists, computed, self.scale, self.basis, language);
        let own = ComputedStyle {
            edges: EdgesGroup {
                border: EdgesGroup::INITIAL.border,
                padding: EdgesGroup::INITIAL.padding,
                ..own.edges
            },
            paints: false,
            ..own
        };
        let size = self.measured(node_id.as_u64());
        builder.float(NodeKey(node_id.as_u64()), &own, side, size);
    }

    /// Asks for the block's `::first-letter` before `text`, in the style the
    /// pseudo-element has in `parent`, the box the text is in, until the
    /// letter is found.
    ///
    /// The builder finds the letter as the text is written; it is found once
    /// a text holds anything but white space and punctuation, and asking
    /// again after is ignored, so the asking stops there.
    fn arm_first_letter(
        &mut self,
        builder: &mut LayoutBuilder<'_>,
        parent: Parent<'_>,
        text: &str,
    ) {
        let Some(pseudo) = &self.first_letter else {
            return;
        };
        let letter = if parent.is_root {
            pseudo.clone()
        } else {
            self.cascade
                .first_letter(self.root, pseudo, parent.computed)
        };
        let letter_first_line = parent
            .first_line
            .map(|first_line| self.cascade.first_letter(self.root, pseudo, first_line));
        {
            let lists = style::FontLists::of(&letter, &self.feature_values);
            let style =
                style::computed_style(&lists, &letter, self.scale, self.basis, parent.language);
            let first_line_lists = letter_first_line
                .as_deref()
                .map(|computed| style::FontLists::of(computed, &self.feature_values));
            let first_line_style = match (&letter_first_line, &first_line_lists) {
                (Some(computed), Some(lists)) => Some(style::computed_style(
                    lists,
                    computed,
                    self.scale,
                    self.basis,
                    parent.language,
                )),
                _ => None,
            };
            builder.set_first_letter(
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

/// What an inline box is opened as.
#[derive(Copy, Clone, PartialEq)]
enum BoxKind {
    Span,
    Ruby,
    Annotation,
    /// No box of its own: `display: contents`, or an annotation container
    /// around `<rt>`s.
    Contents,
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
