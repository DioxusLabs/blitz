use crate::Document;
use crate::layout::damage::HoistedPaintChildren;
use bitflags::bitflags;
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, DomEventData, HitResult, PointerCoords,
};
use blitz_traits::node_id::NodeId;
use blitz_traits::shell::ShellProvider;
use euclid::{Point2D, Rect, Size2D};
use html_escape::encode_quoted_attribute_to_string;
use keyboard_types::Modifiers;
use kurbo::{Affine, Rect as KurboRect};
use markup5ever::{LocalName, local_name};
use parley::{BreakReason, Cluster, ClusterSide};
use selectors::matching::ElementSelectorFlags;
use std::cell::{Cell, RefCell};
use std::fmt::Write;
use std::ops::Deref;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use style::invalidation::element::restyle_hints::RestyleHint;
use style::properties::ComputedValues;
use style::properties::generated::longhands::position::computed_value::T as Position;
use style::selector_parser::RestyleDamage;
use style::servo_arc::Arc as ServoArc;
use style::shared_lock::SharedRwLock;
use style::stylesheets::UrlExtraData;
use style::values::computed::CSSPixelLength;
use style::values::computed::Display as StyloDisplay;
use style::values::specified::box_::{DisplayInside, DisplayOutside};
use style_dom::ElementState;
use style_traits::values::ToCss;
use taffy::{Cache, prelude::Layout};
use thin_vec::ThinVec;

use super::stylo_data::{ComputedStyleRef, StyloData};
use super::{Attribute, DocumentData, ElementData, LayoutData};

#[derive(Clone, Copy)]
enum OutputStyle {
    Normal,
    Pretty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayOuter {
    Block,
    Inline,
    None,
}

bitflags! {
    #[derive(Clone, Copy, PartialEq)]
    pub struct NodeFlags: u32 {
        /// Whether the node is the root node of an Inline Formatting Context
        const IS_INLINE_ROOT = 0b00000001;
        /// Whether the node is the root node of an Table formatting context
        const IS_TABLE_ROOT = 0b00000010;
        /// Whether the node is "in the document" (~= has a parent and isn't a template node)
        const IS_IN_DOCUMENT = 0b00000100;
    }
}

impl NodeFlags {
    #[inline(always)]
    pub fn is_inline_root(&self) -> bool {
        self.contains(Self::IS_INLINE_ROOT)
    }

    #[inline(always)]
    pub fn is_table_root(&self) -> bool {
        self.contains(Self::IS_TABLE_ROOT)
    }

    #[inline(always)]
    pub fn is_in_document(&self) -> bool {
        self.contains(Self::IS_IN_DOCUMENT)
    }

    #[inline(always)]
    pub fn reset_construction_flags(&mut self) {
        self.remove(Self::IS_INLINE_ROOT);
        self.remove(Self::IS_TABLE_ROOT);
    }
}

pub struct Node {
    // The actual tree we belong to. This is unsafe!!
    tree: *mut crate::NodeTree,

    /// Our Id
    pub id: NodeId,
    /// Our parent's ID
    pub parent: Option<NodeId>,
    // What are our children?
    pub children: ThinVec<NodeId>,
    /// Our parent in the layout hierachy: a separate list that includes anonymous collections of inline elements
    pub layout_parent: Cell<Option<NodeId>>,
    /// A separate child list that includes anonymous collections of inline elements
    pub layout_children: RefCell<Option<ThinVec<NodeId>>>,
    /// Anonymous block boxes created for this node during layout construction.
    ///
    /// Anonymous blocks live only in the slab (they are not part of the DOM
    /// `children` list), so we track the ones we own here to be able to
    /// deallocate them when this node is reconstructed.
    pub anonymous_blocks: ThinVec<NodeId>,
    /// The same as layout_children, but sorted by z-index
    pub paint_children: RefCell<Option<ThinVec<NodeId>>>,
    pub stacking_context: Option<Box<HoistedPaintChildren>>,

    // Flags
    pub flags: NodeFlags,

    /// Node type (Element, TextNode, etc) specific data.
    ///
    /// For element nodes this holds the [`ElementData`], which stores most of
    /// the per-node style/layout state. For the document node it holds the
    /// [`DocumentData`]. Access the moved fields through the forwarding methods
    /// on [`Node`] (e.g. [`Node::style`], [`Node::final_layout`]).
    pub data: NodeData,
}

unsafe impl Send for Node {}
unsafe impl Sync for Node {}

/// Generates forwarding accessors for fields that live on both [`ElementData`]
/// (element / anonymous block nodes) and [`DocumentData`] (the document node).
macro_rules! universal_accessors {
    ($($(#[$meta:meta])* $field:ident / $field_mut:ident : $ty:ty),* $(,)?) => {
        impl Node {
            $(
                $(#[$meta])*
                #[inline]
                pub fn $field(&self) -> &$ty {
                    match &self.data {
                        NodeData::Element(data) | NodeData::AnonymousBlock(data) => &data.$field,
                        NodeData::Document(data) => &data.$field,
                        _ => panic!(concat!("`", stringify!($field), "` is not available on this node kind")),
                    }
                }

                $(#[$meta])*
                #[inline]
                pub fn $field_mut(&mut self) -> &mut $ty {
                    match &mut self.data {
                        NodeData::Element(data) | NodeData::AnonymousBlock(data) => &mut data.$field,
                        NodeData::Document(data) => &mut data.$field,
                        _ => panic!(concat!("`", stringify!($field), "` is not available on this node kind")),
                    }
                }
            )*
        }
    };
}

universal_accessors! {
    stylo_element_data / stylo_element_data_mut: StyloData,
    transform / transform_mut: Option<Box<Affine>>,
    display_constructed_as / display_constructed_as_mut: StyloDisplay,
    // The document node is styled/snapshotted like an element, so it also
    // carries these:
    element_state / element_state_mut: ElementState,
    snapshot_handled / snapshot_handled_mut: AtomicBool,
    // `apply_selector_flags` deposits `for_parent()` flags on the parent node,
    // and the parent of the root <html> element is the document -- so the
    // document has to be able to hold selector flags too.
    selector_flags / selector_flags_mut: Cell<ElementSelectorFlags>,
}

impl Node {
    /// This node's layout output state, or a shared default if layout has
    /// never written to this node.
    ///
    /// Panics for node kinds which do not participate in layout (text and
    /// comment nodes).
    #[inline]
    pub fn layout_data(&self) -> &LayoutData {
        match &self.data {
            NodeData::Element(data) | NodeData::AnonymousBlock(data) => data.layout_data(),
            NodeData::Document(data) => data.layout_data(),
            _ => panic!("`layout_data` is not available on this node kind"),
        }
    }

    /// Mutable access to this node's layout output state, allocating it if it
    /// does not yet exist.
    ///
    /// Panics for node kinds which do not participate in layout (text and
    /// comment nodes).
    #[inline]
    pub fn layout_data_mut(&mut self) -> &mut LayoutData {
        match &mut self.data {
            NodeData::Element(data) | NodeData::AnonymousBlock(data) => data.layout_data_mut(),
            NodeData::Document(data) => data.layout_data_mut(),
            _ => panic!("`layout_data` is not available on this node kind"),
        }
    }

    /// Mutable access to this node's layout output state, if it has been
    /// allocated. Never allocates. Returns `None` for node kinds which do not
    /// participate in layout.
    #[inline]
    pub fn try_layout_data_mut(&mut self) -> Option<&mut LayoutData> {
        match &mut self.data {
            NodeData::Element(data) | NodeData::AnonymousBlock(data) => {
                data.layout_data.as_deref_mut()
            }
            NodeData::Document(data) => data.layout_data.as_deref_mut(),
            _ => None,
        }
    }

    /// Clear this node's taffy layout cache without allocating `LayoutData`
    /// for nodes that have never been laid out.
    #[inline]
    pub fn clear_layout_cache(&mut self) {
        if let Some(layout_data) = self.try_layout_data_mut() {
            layout_data.cache.clear();
        }
    }

    #[inline]
    pub fn cache(&self) -> &Cache {
        &self.layout_data().cache
    }

    #[inline]
    pub fn cache_mut(&mut self) -> &mut Cache {
        &mut self.layout_data_mut().cache
    }

    #[inline]
    pub fn unrounded_layout(&self) -> &Layout {
        &self.layout_data().unrounded_layout
    }

    #[inline]
    pub fn unrounded_layout_mut(&mut self) -> &mut Layout {
        &mut self.layout_data_mut().unrounded_layout
    }

    #[inline]
    pub fn final_layout(&self) -> &Layout {
        &self.layout_data().final_layout
    }

    #[inline]
    pub fn final_layout_mut(&mut self) -> &mut Layout {
        &mut self.layout_data_mut().final_layout
    }

    #[inline]
    pub fn scroll_offset(&self) -> &crate::Point<f64> {
        &self.layout_data().scroll_offset
    }

    #[inline]
    pub fn scroll_offset_mut(&mut self) -> &mut crate::Point<f64> {
        &mut self.layout_data_mut().scroll_offset
    }

    #[inline]
    pub fn scrollable_overflow(&self) -> &KurboRect {
        &self.layout_data().scrollable_overflow
    }

    #[inline]
    pub fn scrollable_overflow_mut(&mut self) -> &mut KurboRect {
        &mut self.layout_data_mut().scrollable_overflow
    }

    /// Style data from stylo, if this node kind carries it (element or document
    /// nodes). Returns `None` for text/comment nodes.
    #[inline]
    pub fn try_stylo_element_data(&self) -> Option<&StyloData> {
        match &self.data {
            NodeData::Element(data) | NodeData::AnonymousBlock(data) => {
                Some(&data.stylo_element_data)
            }
            NodeData::Document(data) => Some(&data.stylo_element_data),
            _ => None,
        }
    }

    #[inline]
    pub fn try_stylo_element_data_mut(&mut self) -> Option<&mut StyloData> {
        match &mut self.data {
            NodeData::Element(data) | NodeData::AnonymousBlock(data) => {
                Some(&mut data.stylo_element_data)
            }
            NodeData::Document(data) => Some(&mut data.stylo_element_data),
            _ => None,
        }
    }

    /// The `dirty_descendants` flag, if this node kind carries it (element or
    /// document nodes). Returns `None` for text/comment nodes.
    #[inline]
    fn dirty_descendants_flag(&self) -> Option<&AtomicBool> {
        match &self.data {
            NodeData::Element(data) | NodeData::AnonymousBlock(data) => {
                Some(&data.dirty_descendants)
            }
            NodeData::Document(data) => Some(&data.dirty_descendants),
            _ => None,
        }
    }

    /// The `damaged_descendants` flag, if this node kind carries it (element or
    /// document nodes). Returns `None` for text/comment nodes.
    #[inline]
    fn damaged_descendants_flag(&self) -> Option<&AtomicBool> {
        match &self.data {
            NodeData::Element(data) | NodeData::AnonymousBlock(data) => {
                Some(&data.damaged_descendants)
            }
            NodeData::Document(data) => Some(&data.damaged_descendants),
            _ => None,
        }
    }

    /// The document's shared style lock. Only available on element and
    /// document nodes.
    #[inline]
    pub fn guard(&self) -> &SharedRwLock {
        let guard = match &self.data {
            NodeData::Element(data) | NodeData::AnonymousBlock(data) => data.guard.as_ref(),
            NodeData::Document(data) => data.guard.as_ref(),
            _ => None,
        };
        guard.expect("`guard` is not available on this node kind")
    }

    #[inline]
    pub fn has_snapshot(&self) -> bool {
        match &self.data {
            NodeData::Element(data) | NodeData::AnonymousBlock(data) => data.has_snapshot,
            NodeData::Document(data) => data.has_snapshot,
            _ => false,
        }
    }

    #[inline]
    pub fn set_has_snapshot(&mut self, value: bool) {
        match &mut self.data {
            NodeData::Element(data) | NodeData::AnonymousBlock(data) => data.has_snapshot = value,
            NodeData::Document(data) => data.has_snapshot = value,
            _ => {}
        }
    }

    #[inline]
    pub fn before(&self) -> Option<NodeId> {
        self.element_data().and_then(|data| data.before)
    }

    #[inline]
    pub fn after(&self) -> Option<NodeId> {
        self.element_data().and_then(|data| data.after)
    }
}

impl Node {
    pub(crate) fn new(
        tree: *mut crate::NodeTree,
        id: NodeId,
        guard: SharedRwLock,
        mut data: NodeData,
    ) -> Self {
        // Store a handle to the document's shared style lock on the node data.
        // Both element and document nodes are styled by stylo and so need it.
        match &mut data {
            NodeData::Element(data) | NodeData::AnonymousBlock(data) => {
                data.guard = Some(guard);
            }
            NodeData::Document(data) => data.guard = Some(guard),
            _ => {}
        }

        Self {
            tree,

            id,
            parent: None,
            children: ThinVec::new(),
            layout_parent: Cell::new(None),
            layout_children: RefCell::new(None),
            anonymous_blocks: ThinVec::new(),
            paint_children: RefCell::new(None),
            stacking_context: None,

            flags: NodeFlags::empty(),
            data,
        }
    }

    pub fn set_transform(&mut self, scale: f32) -> Option<Affine> {
        let transform = self.primary_styles().and_then(|s| {
            let size = self.final_layout().size;
            let reference_box = Rect::new(
                Point2D::new(CSSPixelLength::new(0.0), CSSPixelLength::new(0.0)),
                Size2D::new(
                    CSSPixelLength::new(size.width),
                    CSSPixelLength::new(size.height),
                ),
            );
            // Resolve the transform in CSS pixels, then convert it to device-pixel space
            // (S * T * S^-1): translation components are scaled, linear components are not.
            crate::resolve_2d_transform(s.get_box(), reference_box).map(|t| {
                let scale = scale as f64;
                let [m11, m12, m21, m22, m41, m42] = t.as_coeffs();
                Affine::new([m11, m12, m21, m22, m41 * scale, m42 * scale])
            })
        });

        let slot = self.transform_mut();
        match (slot.as_deref_mut(), transform) {
            (Some(existing), Some(new)) => *existing = new,
            (None, Some(new)) => *slot = Some(Box::new(new)),
            (_, None) => *slot = None,
        }
        transform
    }

    pub fn pe_by_index(&self, index: usize) -> Option<NodeId> {
        match index {
            0 => self.after(),
            1 => self.before(),
            _ => panic!("Invalid pseudo element index"),
        }
    }

    pub fn set_pe_by_index(&mut self, index: usize, value: Option<NodeId>) {
        let Some(data) = self.element_data_mut() else {
            return;
        };
        match index {
            0 => data.after = value,
            1 => data.before = value,
            _ => panic!("Invalid pseudo element index"),
        }
    }

    pub(crate) fn display_style(&self) -> Option<StyloDisplay> {
        Some(self.primary_styles().as_ref()?.clone_display())
    }

    pub fn is_or_contains_block(&self) -> bool {
        let style = self.primary_styles();
        let style = style.as_ref();

        // Ignore out-of-flow items
        let position = style
            .map(|s| s.clone_position())
            .unwrap_or(Position::Relative);
        let is_in_flow = matches!(
            position,
            Position::Static | Position::Relative | Position::Sticky
        );
        if !is_in_flow {
            return false;
        }
        // Floated boxes do not break up the inline flow: they participate in the
        // inline formatting context as out-of-flow inline boxes
        let is_floating = style
            .map(|s| s.clone_float().is_floating())
            .unwrap_or(false);
        if is_floating {
            return false;
        }
        let display = style
            .map(|s| s.clone_display())
            .unwrap_or(StyloDisplay::inline());
        match display.outside() {
            DisplayOutside::None => false,
            DisplayOutside::Block => true,
            _ => {
                if display.inside() == DisplayInside::Flow {
                    self.children
                        .iter()
                        .copied()
                        .any(|child_id| self.tree()[child_id].is_or_contains_block())
                } else {
                    false
                }
            }
        }
    }

    pub fn is_whitespace_node(&self) -> bool {
        match &self.data {
            NodeData::Text(data) => data.content.chars().all(|c| c.is_ascii_whitespace()),
            _ => false,
        }
    }

    pub fn is_focussable(&self) -> bool {
        self.data
            .downcast_element()
            .map(|el| el.is_focussable)
            .unwrap_or(false)
    }

    pub fn set_restyle_hint(&mut self, hint: RestyleHint) {
        if let Some(stylo_element_data) = self.try_stylo_element_data_mut() {
            if let Some(mut element_data) = stylo_element_data.get_mut() {
                element_data.hint.insert(hint);
            }
        }
        // Mark all ancestors as having dirty descendants so the style traversal
        // will visit this node's subtree
        self.mark_ancestors_dirty();
    }

    /// Returns whether this node has any descendants that need restyling.
    pub fn has_dirty_descendants(&self) -> bool {
        self.dirty_descendants_flag()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
    }

    /// Sets the dirty_descendants flag on this node.
    pub fn set_dirty_descendants(&self) {
        if let Some(flag) = self.dirty_descendants_flag() {
            flag.store(true, Ordering::Relaxed);
        }
    }

    /// Clears the dirty_descendants flag on this node.
    pub fn unset_dirty_descendants(&self) {
        if let Some(flag) = self.dirty_descendants_flag() {
            flag.store(false, Ordering::Relaxed);
        }
    }

    /// Marks all ancestors of this node as having dirty descendants.
    /// This propagates the dirty flag up the tree so that the style traversal
    /// knows to visit the subtree containing this node.
    pub fn mark_ancestors_dirty(&self) {
        let mut current_id = self.parent;
        while let Some(parent_id) = current_id {
            let parent = &self.tree()[parent_id];
            // If this ancestor already has dirty_descendants set, we can stop
            // because all further ancestors must also have it set
            if let Some(flag) = parent.dirty_descendants_flag() {
                if flag.swap(true, Ordering::Relaxed) {
                    break;
                }
            }
            current_id = parent.parent;
        }
    }

    /// Returns whether this node or any of its descendants may carry damage.
    pub fn has_damaged_descendants(&self) -> bool {
        self.damaged_descendants_flag()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
    }

    /// Clears the damaged_descendants flag on this node.
    pub fn unset_damaged_descendants(&self) {
        if let Some(flag) = self.damaged_descendants_flag() {
            flag.store(false, Ordering::Relaxed);
        }
    }

    /// Marks this node and all of its ancestors as (potentially) carrying
    /// damage, so that the damage propagation pass visits this node's subtree.
    ///
    /// The invariant is: if a node carries damage (or needs damage-phase
    /// processing such as pseudo-element style syncing), then it and all of
    /// its ancestors have `damaged_descendants` set.
    pub fn mark_damaged(&self) {
        if let Some(flag) = self.damaged_descendants_flag() {
            if flag.swap(true, Ordering::Relaxed) {
                return;
            }
        }
        let mut current_id = self.parent;
        while let Some(parent_id) = current_id {
            let parent = &self.tree()[parent_id];
            // If this ancestor already has damaged_descendants set, we can stop
            // because all further ancestors must also have it set
            if let Some(flag) = parent.damaged_descendants_flag() {
                if flag.swap(true, Ordering::Relaxed) {
                    break;
                }
            }
            current_id = parent.parent;
        }
    }

    // pub fn damage_mut(&mut self) -> Option<&mut RestyleDamage> {
    //     self.stylo_element_data
    //         .get_mut()
    //         .map(|mut data: ElementDataMut<'a>| &'a mut data.damage)
    // }

    pub fn damage(&self) -> Option<RestyleDamage> {
        self.try_stylo_element_data()
            .and_then(|stylo| stylo.get().map(|data| data.damage))
    }

    pub fn set_damage(&mut self, damage: RestyleDamage) {
        if let Some(stylo) = self.try_stylo_element_data_mut() {
            if let Some(mut data) = stylo.get_mut() {
                data.damage = damage;
            }
        }
    }

    pub fn insert_damage(&mut self, damage: RestyleDamage) {
        if let Some(stylo) = self.try_stylo_element_data_mut() {
            if let Some(mut data) = stylo.get_mut() {
                data.damage |= damage;
            }
        }
        if !damage.is_empty() {
            self.mark_damaged();
        }
    }

    pub fn remove_damage(&mut self, damage: RestyleDamage) {
        if let Some(stylo) = self.try_stylo_element_data_mut() {
            if let Some(mut data) = stylo.get_mut() {
                data.damage.remove(damage);
            }
        }
    }

    pub fn clear_damage_mut(&mut self) {
        if let Some(stylo) = self.try_stylo_element_data_mut() {
            if let Some(mut data) = stylo.get_mut() {
                data.damage = RestyleDamage::empty();
            }
        }
    }

    // State changes (hover/focus/active/disabled) do not set a restyle hint.
    // Invalidation is driven by element snapshots: the style traversal diffs the
    // snapshotted (pre-change) state against the current state and invalidates
    // only the elements matched by selectors that depend on the changed state
    // bits. Ancestors are marked dirty so the traversal reaches this node.
    pub fn hover(&mut self) {
        if let Some(data) = self.element_data_mut() {
            data.element_state.insert(ElementState::HOVER);
        }
        self.mark_ancestors_dirty();
    }

    pub fn unhover(&mut self) {
        if let Some(data) = self.element_data_mut() {
            data.element_state.remove(ElementState::HOVER);
        }
        self.mark_ancestors_dirty();
    }

    pub fn is_hovered(&self) -> bool {
        self.element_data()
            .is_some_and(|data| data.element_state.contains(ElementState::HOVER))
    }

    pub fn focus(&mut self, shell_provider: Arc<dyn ShellProvider>) {
        if let Some(data) = self.element_data_mut() {
            data.element_state
                .insert(ElementState::FOCUS | ElementState::FOCUSRING);
        }
        self.mark_ancestors_dirty();

        // If focussing a text input, enable IME and set IME area
        if self
            .element_data()
            .and_then(|elem| elem.text_input_data())
            .is_some()
        {
            shell_provider.set_ime_enabled(true);
            let mut pos = self.absolute_position(0.0, 0.0);
            pos.x += self.final_layout().content_box_x();
            pos.y += self.final_layout().content_box_y();
            let width = self.final_layout().content_box_width();
            let height = self.final_layout().content_box_height();
            shell_provider.set_ime_cursor_area(pos.x, pos.y, width, height);
        }
    }

    pub fn blur(&mut self, shell_provider: Arc<dyn ShellProvider>) {
        if let Some(data) = self.element_data_mut() {
            data.element_state
                .remove(ElementState::FOCUS | ElementState::FOCUSRING);
        }
        self.mark_ancestors_dirty();

        // If blurring a text input, disable IME
        if self
            .element_data()
            .and_then(|elem| elem.text_input_data())
            .is_some()
        {
            shell_provider.set_ime_enabled(false);
        }
    }

    pub fn is_focussed(&self) -> bool {
        self.element_data()
            .is_some_and(|data| data.element_state.contains(ElementState::FOCUS))
    }

    pub fn active(&mut self) {
        if let Some(data) = self.element_data_mut() {
            data.element_state.insert(ElementState::ACTIVE);
        }
        self.mark_ancestors_dirty();
    }

    pub fn unactive(&mut self) {
        if let Some(data) = self.element_data_mut() {
            data.element_state.remove(ElementState::ACTIVE);
        }
        self.mark_ancestors_dirty();
    }

    pub fn is_active(&self) -> bool {
        self.element_data()
            .is_some_and(|data| data.element_state.contains(ElementState::ACTIVE))
    }

    // Marks the node as disabled if it can be.
    // It does not disable any children which should be disabled as well (relevant for the `select` element).
    pub fn disable(&mut self) {
        if let Some(data) = self.element_data_mut() {
            if data.can_be_disabled() {
                data.element_state.insert(ElementState::DISABLED);
                data.element_state.remove(ElementState::ENABLED);
            }
        }
        self.mark_ancestors_dirty();
    }

    // Marks the node as enabled if it can be.
    // It does not enable any children which should be enabled as well (relevant for the `select` element).
    pub fn enable(&mut self) {
        if let Some(data) = self.element_data_mut() {
            if data.can_be_disabled() {
                data.element_state.insert(ElementState::ENABLED);
                data.element_state.remove(ElementState::DISABLED);
            }
        }
        self.mark_ancestors_dirty();
    }

    pub fn subdoc(&self) -> Option<&dyn Document> {
        self.element_data().and_then(|el| el.sub_doc_data())
    }

    pub fn subdoc_mut(&mut self) -> Option<&mut dyn Document> {
        self.element_data_mut().and_then(|el| el.sub_doc_data_mut())
    }

    pub fn text_input_v_centering_offset(&self, scale: f64) -> f64 {
        // For single-line inputs, add an offset to vertically center the text input layout
        // within the content box of it's node.
        if let Some(input_data) = self
            .data
            .downcast_element()
            .and_then(|el| el.text_input_data())
        {
            if !input_data.is_multiline {
                let content_box_height = self.final_layout().content_box_height();
                let input_height = input_data.editor.try_layout().unwrap().height() / scale as f32;
                let y_offset = ((content_box_height - input_height) / 2.0).max(0.0);

                return y_offset as f64;
            }
        }

        0.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NodeKind {
    Document,
    Element,
    AnonymousBlock,
    Text,
    Comment,
}

/// The different kinds of nodes in the DOM.
#[derive(Debug, Clone)]
pub enum NodeData {
    /// The `Document` itself - the root node of a HTML document.
    Document(Box<DocumentData>),

    /// An element with attributes.
    Element(Box<ElementData>),

    /// An anonymous block box
    AnonymousBlock(Box<ElementData>),

    /// A text node.
    Text(TextNodeData),

    /// A comment.
    Comment {
        /// The textual content of the comment
        contents: String,
    },
    // /// A `DOCTYPE` with name, public id, and system id. See
    // /// [document type declaration on wikipedia][https://en.wikipedia.org/wiki/Document_type_declaration]
    // Doctype { name: String, public_id: String, system_id: String },

    // /// A Processing instruction.
    // ProcessingInstruction { target: String, contents: String },
}

impl NodeData {
    pub fn downcast_element(&self) -> Option<&ElementData> {
        match self {
            Self::Element(data) => Some(data),
            Self::AnonymousBlock(data) => Some(data),
            _ => None,
        }
    }

    pub fn downcast_element_mut(&mut self) -> Option<&mut ElementData> {
        match self {
            Self::Element(data) => Some(data),
            Self::AnonymousBlock(data) => Some(data),
            _ => None,
        }
    }

    pub fn is_element_with_tag_name(&self, name: &impl PartialEq<LocalName>) -> bool {
        let Some(elem) = self.downcast_element() else {
            return false;
        };
        *name == elem.name.local
    }

    pub fn attrs(&self) -> Option<&[Attribute]> {
        Some(&self.downcast_element()?.attrs)
    }

    pub fn attr(&self, name: impl PartialEq<LocalName>) -> Option<&str> {
        self.downcast_element()?.attr(name)
    }

    pub fn has_attr(&self, name: impl PartialEq<LocalName>) -> bool {
        self.downcast_element()
            .is_some_and(|elem| elem.has_attr(name))
    }

    pub fn kind(&self) -> NodeKind {
        match self {
            NodeData::Document(_) => NodeKind::Document,
            NodeData::Element(_) => NodeKind::Element,
            NodeData::AnonymousBlock(_) => NodeKind::AnonymousBlock,
            NodeData::Text(_) => NodeKind::Text,
            NodeData::Comment { .. } => NodeKind::Comment,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TextNodeData {
    /// The textual content of the text node
    pub content: String,
}

impl TextNodeData {
    pub fn new(content: String) -> Self {
        Self { content }
    }
}

/*
-> Computed styles
-> Layout
-----> Needs to happen only when styles are computed
*/

// type DomRefCell<T> = RefCell<T>;

// pub struct DomData {
//     // ... we can probs just get away with using the html5ever types directly. basically just using the servo dom, but without the bindings
//     local_name: html5ever::LocalName,
//     tag_name: html5ever::QualName,
//     namespace: html5ever::Namespace,
//     prefix: DomRefCell<Option<html5ever::Prefix>>,
//     attrs: DomRefCell<Vec<Attr>>,
//     // attrs: DomRefCell<Vec<Dom<Attr>>>,
//     id_attribute: DomRefCell<Option<Atom>>,
//     is: DomRefCell<Option<LocalName>>,
//     // style_attribute: DomRefCell<Option<Arc<Locked<PropertyDeclarationBlock>>>>,
//     // attr_list: MutNullableDom<NamedNodeMap>,
//     // class_list: MutNullableDom<DOMTokenList>,
//     state: Cell<ElementState>,
// }

impl Node {
    pub fn tree(&self) -> &crate::NodeTree {
        unsafe { &*self.tree }
    }

    #[track_caller]
    pub fn with(&self, id: NodeId) -> &Node {
        self.tree().get(id).unwrap()
    }

    pub fn print_tree(&self, level: usize) {
        println!(
            "{} {} {:?} {} {:?}",
            "  ".repeat(level),
            self.id,
            self.parent,
            self.node_debug_str().replace('\n', ""),
            self.children
        );
        // println!("{} {:?}", "  ".repeat(level), self.children);
        for child_id in self.children.iter() {
            let child = self.with(*child_id);
            child.print_tree(level + 1)
        }
    }

    // Get the index of the current node in the parents child list
    pub fn index_of_child(&self, child_id: NodeId) -> Option<usize> {
        self.children.iter().position(|id| *id == child_id)
    }

    // Get the index of the current node in the parents child list
    pub fn child_index(&self) -> Option<usize> {
        self.tree()[self.parent?]
            .children
            .iter()
            .position(|id| *id == self.id)
    }

    // Get the nth node in the parents child list
    pub fn forward(&self, n: usize) -> Option<&Node> {
        let child_idx = self.child_index().unwrap_or(0);
        self.tree()[self.parent?]
            .children
            .get(child_idx + n)
            .map(|id| self.with(*id))
    }

    pub fn backward(&self, n: usize) -> Option<&Node> {
        let child_idx = self.child_index().unwrap_or(0);
        if child_idx < n {
            return None;
        }

        self.tree()[self.parent?]
            .children
            .get(child_idx - n)
            .map(|id| self.with(*id))
    }

    pub fn is_element(&self) -> bool {
        matches!(self.data, NodeData::Element { .. })
    }

    pub fn is_anonymous(&self) -> bool {
        matches!(self.data, NodeData::AnonymousBlock { .. })
    }

    pub fn is_text_node(&self) -> bool {
        matches!(self.data, NodeData::Text { .. })
    }

    pub fn element_data(&self) -> Option<&ElementData> {
        match self.data {
            NodeData::Element(ref data) => Some(data),
            NodeData::AnonymousBlock(ref data) => Some(data),
            _ => None,
        }
    }

    pub fn element_data_mut(&mut self) -> Option<&mut ElementData> {
        match self.data {
            NodeData::Element(ref mut data) => Some(data),
            NodeData::AnonymousBlock(ref mut data) => Some(data),
            _ => None,
        }
    }

    pub fn text_data(&self) -> Option<&TextNodeData> {
        match self.data {
            NodeData::Text(ref data) => Some(data),
            _ => None,
        }
    }

    pub fn text_data_mut(&mut self) -> Option<&mut TextNodeData> {
        match self.data {
            NodeData::Text(ref mut data) => Some(data),
            _ => None,
        }
    }

    pub fn node_debug_str(&self) -> String {
        let mut s = String::new();

        match &self.data {
            NodeData::Document(_) => write!(s, "DOCUMENT"),
            // NodeData::Doctype { name, .. } => write!(s, "DOCTYPE {name}"),
            NodeData::Text(data) => {
                let bytes = data.content.as_bytes();
                write!(
                    s,
                    "TEXT {}",
                    std::str::from_utf8(bytes.split_at(10.min(bytes.len())).0)
                        .unwrap_or("INVALID UTF8")
                )
            }
            NodeData::Comment { .. } => write!(s, "COMMENT"),
            NodeData::AnonymousBlock(_) => write!(s, "AnonymousBlock"),
            NodeData::Element(data) => {
                let name = &data.name;
                let class = self.attr(local_name!("class")).unwrap_or("");
                let id = self.attr(local_name!("id")).unwrap_or("");
                let display = self.display_constructed_as().to_css_string();
                write!(s, "<{}", name.local).unwrap();
                if !id.is_empty() {
                    write!(s, " #{id}").unwrap();
                }
                if !class.is_empty() {
                    if class.contains(' ') {
                        write!(s, " class=\"{class}\"").unwrap()
                    } else {
                        write!(s, " .{class}").unwrap()
                    }
                }
                write!(s, "> ({display})")
            } // NodeData::ProcessingInstruction { .. } => write!(s, "ProcessingInstruction"),
        }
        .unwrap();
        s
    }

    /// Renders the HTML of this node and all its children as a `String` without extra whitespace.
    ///
    /// Example output:
    ///
    /// ```text
    /// <html><head /><body><main id="main"><div class="arbitrary-class" /></main></body></html>
    /// ```
    pub fn outer_html(&self) -> String {
        let mut output = String::new();
        self.write_outer_html(&mut output);
        output
    }

    /// Renders the HTML of this node and all its children as a `String` with whitespace for human
    /// readability.
    ///
    /// Example output:
    ///
    /// ```text
    /// <html>
    ///   <head />
    ///   <body>
    ///     <main id="main">
    ///       <div class="arbitrary-class" />
    ///     </main>
    ///   </body>
    /// </html>
    /// ```
    pub fn outer_html_pretty(&self) -> String {
        let mut output = String::new();
        self.write_outer_html_pretty(&mut output);
        output
    }

    pub fn write_outer_html(&self, writer: &mut String) {
        self.write_outer_html_in_style(writer, OutputStyle::Normal, 0);
    }

    pub fn write_outer_html_pretty(&self, writer: &mut String) {
        self.write_outer_html_in_style(writer, OutputStyle::Pretty, 0);
    }

    fn write_outer_html_in_style(&self, writer: &mut String, style: OutputStyle, nesting: usize) {
        const INDENT: &str = "  ";
        let has_children = !self.children.is_empty();
        let current_color = self
            .primary_styles()
            .map(|style| style.clone_color())
            .map(|color| color.to_css_string());

        match &self.data {
            NodeData::Document(_) => {}
            NodeData::Comment { .. } => {}
            NodeData::AnonymousBlock(_) => {}
            // NodeData::Doctype { name, .. } => write!(s, "DOCTYPE {name}"),
            NodeData::Text(data) => {
                if matches!(style, OutputStyle::Pretty) {
                    for _ in 0..nesting {
                        writer.push_str(INDENT);
                    }
                }
                writer.push_str(data.content.as_str());
                if matches!(style, OutputStyle::Pretty) {
                    writer.push('\n');
                }
            }
            NodeData::Element(data) => {
                if matches!(style, OutputStyle::Pretty) {
                    for _ in 0..nesting {
                        writer.push_str(INDENT);
                    }
                }
                writer.push('<');
                writer.push_str(&data.name.local);

                for attr in data.attrs() {
                    writer.push(' ');
                    writer.push_str(&attr.name.local);
                    writer.push_str("=\"");
                    #[allow(clippy::unnecessary_unwrap)] // Convert to if-let chain once stabilised
                    if current_color.is_some() && attr.value.contains("currentColor") {
                        let value = attr
                            .value
                            .replace("currentColor", current_color.as_ref().unwrap());
                        encode_quoted_attribute_to_string(&value, writer);
                    } else {
                        encode_quoted_attribute_to_string(&attr.value, writer);
                    }
                    writer.push('"');
                }
                if !has_children {
                    writer.push_str(" /");
                }
                writer.push('>');
                if matches!(style, OutputStyle::Pretty) {
                    writer.push('\n');
                }

                if has_children {
                    for &child_id in &self.children {
                        self.tree()[child_id].write_outer_html_in_style(writer, style, nesting + 1);
                    }

                    if matches!(style, OutputStyle::Pretty) {
                        for _ in 0..nesting {
                            writer.push_str(INDENT);
                        }
                    }
                    writer.push_str("</");
                    writer.push_str(&data.name.local);
                    writer.push('>');
                    if matches!(style, OutputStyle::Pretty) {
                        writer.push('\n');
                    }
                }
            }
        }
    }

    pub fn attrs(&self) -> Option<&[Attribute]> {
        Some(&self.element_data()?.attrs)
    }

    pub fn attr(&self, name: LocalName) -> Option<&str> {
        let attr = self.attrs()?.iter().find(|id| id.name.local == name)?;
        Some(&attr.value)
    }

    pub fn primary_styles(&self) -> Option<impl Deref<Target = ServoArc<ComputedValues>>> {
        self.try_stylo_element_data()
            .and_then(|stylo| stylo.primary_styles())
    }

    /// A lazy Taffy style backed by this node's primary stylo style.
    ///
    /// Panics if the node has no computed styles: layout always runs after
    /// styling, and the only unstyled nodes (text, comments, descendants of
    /// `display: none`) are never queried by Taffy.
    pub fn layout_style(&self) -> stylo_taffy::TaffyStyloStyle<ComputedStyleRef<'_>> {
        let styles = self
            .try_stylo_element_data()
            .and_then(|stylo| stylo.computed_styles())
            .expect("layout_style() called on a node without computed styles");

        let mut flags = stylo_taffy::StyleFlags::empty();
        if let Some(el) = self.data.downcast_element() {
            if crate::layout::replaced::is_replaced_element(&el.name.local) {
                flags |= stylo_taffy::StyleFlags::IS_REPLACED;
            }
        }

        stylo_taffy::TaffyStyloStyle::new(styles, flags)
    }

    /// The node's `display` as a [`taffy::Display`]. Returns [`taffy::Display::Block`]
    /// for nodes without computed styles (e.g. text nodes).
    pub fn taffy_display(&self) -> taffy::Display {
        self.primary_styles()
            .map(|s| stylo_taffy::convert::display(s.clone_display()))
            .unwrap_or(taffy::Display::Block)
    }

    pub fn text_content(&self) -> String {
        let mut out = String::new();
        self.write_text_content(&mut out);
        out
    }

    fn write_text_content(&self, out: &mut String) {
        match &self.data {
            NodeData::Text(data) => {
                out.push_str(&data.content);
            }
            NodeData::Element(..) | NodeData::AnonymousBlock(..) => {
                for child_id in self.children.iter() {
                    self.with(*child_id).write_text_content(out);
                }
            }
            _ => {}
        }
    }

    pub fn flush_style_attribute(&mut self, url_extra_data: &UrlExtraData) {
        if let NodeData::Element(ref mut elem_data) = self.data {
            if let Some(guard) = elem_data.guard.clone() {
                elem_data.flush_style_attribute(&guard, url_extra_data);
            }
        }
    }

    pub fn order(&self) -> i32 {
        // ::before/::after pseudos are flex/grid items and honor `order`.
        // They sit first/last in layout_children, and the `order` sort is
        // stable, so ties keep ::before first and ::after last.
        self.primary_styles().map(|s| s.clone_order()).unwrap_or(0)
    }

    pub fn z_index(&self) -> i32 {
        self.primary_styles()
            .map(|s| s.clone_z_index().integer_or(0))
            .unwrap_or(0)
    }

    // https://developer.mozilla.org/en-US/docs/Web/CSS/CSS_positioned_layout/Stacking_context#features_creating_stacking_contexts
    pub fn is_stacking_context_root(&self, is_flex_or_grid_item: bool) -> bool {
        let Some(style) = self.primary_styles() else {
            return false;
        };

        let position = style.clone_position();
        let has_z_index = !style.clone_z_index().is_auto();

        if style.clone_opacity() != 1.0 {
            return true;
        }

        let position_based = match position {
            Position::Fixed | Position::Sticky => true,
            Position::Relative | Position::Absolute => has_z_index,
            Position::Static => has_z_index && is_flex_or_grid_item,
        };
        if position_based {
            return true;
        }

        if self.transform().is_some() {
            return true;
        }

        // TODO: mix-blend-mode
        // TODO: filter
        // TODO: clip-path
        // TODO: mask
        // TODO: isolation
        // TODO: contain

        false
    }

    /// Takes an (x, y) position (relative to the *parent's* top-left corner) and returns:
    ///    - None if the position is outside of this node's bounds
    ///    - Some(HitResult) if the position is within the node but doesn't match any children
    ///    - The result of recursively calling child.hit() on the the child element that is
    ///      positioned at that position if there is one.
    ///
    /// TODO: z-index
    /// (If multiple children are positioned at the position then a random one will be recursed into)
    pub fn hit(&self, x: f32, y: f32, scale: f64) -> Option<HitResult> {
        self.hit_inner(x, y, scale, &mut None)
    }

    /// [`hit`](Self::hit), also resolving the innermost overlay scrollbar
    /// thumb under the point into `scrollbar` during the same descent (so
    /// thumb hit-testing shares the exact coordinate handling — transforms
    /// included — of every other hit test).
    pub(crate) fn hit_inner(
        &self,
        x: f32,
        y: f32,
        scale: f64,
        scrollbar: &mut Option<crate::node::ScrollbarRef>,
    ) -> Option<HitResult> {
        use style::computed_values::pointer_events::T as PointerEvents;
        use style::computed_values::visibility::T as Visibility;
        use style::values::computed::Overflow;

        // Don't hit on visbility:hidden elements
        if let Some(style) = self.primary_styles() {
            if matches!(
                style.clone_visibility(),
                Visibility::Hidden | Visibility::Collapse
            ) {
                return None;
            }
        }

        // pointer-events:none makes this element transparent to hits, but its
        // descendants are still tested (one may restore pointer-events:auto).
        let pointer_events_none = self
            .primary_styles()
            .is_some_and(|style| style.clone_pointer_events() == PointerEvents::None);

        let mut x = x - self.final_layout().location.x + self.scroll_offset().x as f32;
        let mut y = y - self.final_layout().location.y + self.scroll_offset().y as f32;

        if let Some(t) = self.transform().as_deref() {
            let p = t.inverse() * kurbo::Point::new(x as f64 * scale, y as f64 * scale);
            x = (p.x / scale) as f32;
            y = (p.y / scale) as f32;
        }

        let size = self.final_layout().size;
        let matches_self = !(x < 0.0
            || x > size.width + self.scroll_offset().x as f32
            || y < 0.0
            || y > size.height + self.scroll_offset().y as f32);

        let overflow_rect = self.final_layout().scrollable_overflow_rect;
        let matches_content = !(x < 0.0
            || x > overflow_rect.right + self.scroll_offset().x as f32
            || y < 0.0
            || y > overflow_rect.bottom + self.scroll_offset().y as f32);

        let matches_hoisted_content = match &self.stacking_context {
            Some(sc) => {
                let content_area = sc.content_area;
                x >= content_area.left + self.scroll_offset().x as f32
                    && x <= content_area.right + self.scroll_offset().x as f32
                    && y >= content_area.top + self.scroll_offset().y as f32
                    && y <= content_area.bottom + self.scroll_offset().y as f32
            }
            None => false,
        };

        // `scrollable_overflow` is stored in device (scaled) pixels, whereas the
        // coordinates here are in CSS pixels, so unscale it before comparing.
        let overflow = *self.scrollable_overflow();

        let matches_overflow = x >= (overflow.x0 / scale) as f32
            && x <= (overflow.x1 / scale) as f32
            && y >= (overflow.y0 / scale) as f32
            && y <= (overflow.y1 / scale) as f32;

        if !matches_self && !matches_content && !matches_hoisted_content && !matches_overflow {
            return None;
        }

        // When overflow is hidden or clip on an axis, children are visually
        // clipped to the border box. A child with a transform may escape the
        // clip region geometrically, so we must prevent hit testing from
        // matching it. If the point is outside the clip region on a clipped
        // axis, we skip child hit testing entirely (but still allow the node
        // itself to be hit, e.g. for scrollbar interaction).
        let hit_children = !self.primary_styles().is_some_and(|style| {
            let outside_x = matches!(style.clone_overflow_x(), Overflow::Hidden | Overflow::Clip)
                && (x < 0.0 || x > size.width + self.scroll_offset().x as f32);
            let outside_y = matches!(style.clone_overflow_y(), Overflow::Hidden | Overflow::Clip)
                && (y < 0.0 || y > size.height + self.scroll_offset().y as f32);
            outside_x || outside_y
        });

        // Descendants overwrite, so the innermost scroll container's thumb
        // wins. Thumb coords are border-box relative (unscrolled).
        if matches_self
            && let Some(sb) = self.scrollbar_at_local(
                (x - self.scroll_offset().x as f32) as f64,
                (y - self.scroll_offset().y as f32) as f64,
            )
        {
            *scrollbar = Some(sb);
        }

        if hit_children {
            if self.flags.is_inline_root() {
                let content_box_offset = taffy::Point {
                    x: self.final_layout().padding.left + self.final_layout().border.left,
                    y: self.final_layout().padding.top + self.final_layout().border.top,
                };
                x -= content_box_offset.x;
                y -= content_box_offset.y;
            }

            // Positive z_index hoisted children
            if matches_hoisted_content {
                if let Some(hoisted) = &self.stacking_context {
                    for hoisted_child in hoisted.pos_z_hoisted_children().rev() {
                        let x = x - hoisted_child.position.x;
                        let y = y - hoisted_child.position.y;
                        if let Some(hit) = self
                            .with(hoisted_child.node_id)
                            .hit_inner(x, y, scale, scrollbar)
                        {
                            return Some(hit);
                        }
                    }
                }
            }

            // Call `.hit()` on each child in turn. If any return `Some` then return that value. Else return `Some(self.id).
            for child_id in self.paint_children.borrow().iter().flatten().rev() {
                if let Some(hit) = self.with(*child_id).hit_inner(x, y, scale, scrollbar) {
                    return Some(hit);
                }
            }

            // Negative z_index hoisted children
            if matches_hoisted_content {
                if let Some(hoisted) = &self.stacking_context {
                    for hoisted_child in hoisted.neg_z_hoisted_children().rev() {
                        let x = x - hoisted_child.position.x;
                        let y = y - hoisted_child.position.y;
                        if let Some(hit) = self
                            .with(hoisted_child.node_id)
                            .hit_inner(x, y, scale, scrollbar)
                        {
                            return Some(hit);
                        }
                    }
                }
            }

            // Inline children
            if self.flags.is_inline_root() {
                let element_data = &self.element_data().unwrap();
                if let Some(ild) = element_data.inline_layout_data.as_ref() {
                    let layout = &ild.layout;
                    let scale = layout.scale();

                    if let Some((cluster, _side)) =
                        Cluster::from_point_exact(layout, x * scale, y * scale)
                    {
                        let style_index = cluster.glyphs().next()?.style_index();
                        let node_id = layout.styles()[style_index].brush.id;
                        let text_pointer_events_none =
                            self.with(node_id).primary_styles().is_some_and(|style| {
                                style.clone_pointer_events() == PointerEvents::None
                            });
                        if !text_pointer_events_none {
                            return Some(HitResult {
                                node_id,
                                x,
                                y,
                                is_text: true,
                            });
                        }
                    }
                }
            }
        }

        // Self (this node)
        if matches_self && !pointer_events_none {
            return Some(HitResult {
                node_id: self.id,
                x,
                y,
                is_text: false,
            });
        }

        None
    }

    /// Find the inline root ancestor of this node (or self if this is an inline root).
    /// Returns None if no inline root ancestor exists.
    pub fn inline_root_ancestor(&self) -> Option<&Node> {
        let mut node = self;
        loop {
            if node.flags.is_inline_root() {
                return Some(node);
            }
            let id = node.layout_parent.get()?;
            node = self.with(id);
        }
    }

    /// Get the text byte offset at a given point, using coordinates already transformed
    /// to be relative to this inline root's content box.
    /// Returns Some(byte_offset) if the point hits text, None otherwise.
    pub fn text_offset_at_point(&self, x: f32, y: f32) -> Option<usize> {
        if !self.flags.is_inline_root() {
            return None;
        }

        let element_data = self.element_data()?;
        let inline_layout = element_data.inline_layout_data.as_ref()?;
        let layout = &inline_layout.layout;
        let scale = layout.scale();

        // Use Parley's cluster hit testing (from_point is more forgiving than from_point_exact)
        let (cluster, side) = Cluster::from_point(layout, x * scale, y * scale)?;

        // Determine byte offset based on which side of the cluster was clicked
        // For LTR text: left side = start of cluster, right side = end of cluster
        // For RTL text: left side = end of cluster, right side = start of cluster
        // Also, explicit line breaks should always use start to avoid cursor appearing on next line
        let is_leading = side == ClusterSide::Left;
        let offset = if cluster.is_rtl() {
            if is_leading {
                cluster.text_range().end
            } else {
                cluster.text_range().start
            }
        } else {
            // LTR text
            if is_leading || cluster.is_line_break() == Some(BreakReason::Explicit) {
                cluster.text_range().start
            } else {
                cluster.text_range().end
            }
        };

        Some(offset)
    }

    /// Computes the Document-relative coordinates of the `Node`
    pub fn absolute_position(&self, x: f32, y: f32) -> crate::util::Point<f32> {
        let x = x + self.final_layout().location.x - self.scroll_offset().x as f32;
        let y = y + self.final_layout().location.y - self.scroll_offset().y as f32;

        // Recurse up the layout hierarchy
        self.layout_parent
            .get()
            .map(|i| self.with(i).absolute_position(x, y))
            .unwrap_or(crate::util::Point { x, y })
    }

    /// Whether this node can act as an [`offset_parent`](Self::offset_parent): a positioned
    /// element, or one of the elements that always qualify (`body`, `td`, `th`).
    fn is_offset_parent(&self) -> bool {
        let Some(styles) = self.primary_styles() else {
            return false;
        };
        if styles.get_box().position != Position::Static {
            return true;
        }
        self.data.is_element_with_tag_name(&local_name!("body"))
            || self.data.is_element_with_tag_name(&local_name!("td"))
            || self.data.is_element_with_tag_name(&local_name!("th"))
    }

    /// Whether this node is a non-positioned `body` element. When such an element is the
    /// `offsetParent`, `offsetLeft`/`offsetTop` are measured from the initial containing
    /// block origin rather than from the `body`'s padding edge.
    fn is_static_body(&self) -> bool {
        self.data.is_element_with_tag_name(&local_name!("body"))
            && self
                .primary_styles()
                .is_some_and(|styles| styles.get_box().position == Position::Static)
    }

    /// The nearest layout ancestor that [is an offset parent](Self::is_offset_parent), as in
    /// CSSOM View's `offsetParent`.
    pub fn offset_parent(&self) -> Option<&Node> {
        let mut node = self;
        loop {
            node = self.with(node.layout_parent.get()?);
            if node.is_offset_parent() {
                return Some(node);
            }
        }
    }

    /// CSSOM View's `offsetLeft`/`offsetTop`: the offset of this node's border box from the
    /// padding edge of its [`offset_parent`](Self::offset_parent).
    pub fn offset_top_left(&self) -> crate::util::Point<f32> {
        let mut x = 0.0;
        let mut y = 0.0;
        let mut current = self;
        loop {
            let layout = current.final_layout();
            x += layout.location.x;
            y += layout.location.y;

            let Some(parent_id) = current.layout_parent.get() else {
                break;
            };
            let parent = self.with(parent_id);
            if parent.is_offset_parent() && !parent.is_static_body() {
                let border = parent.final_layout().border;
                x -= border.left;
                y -= border.top;
                break;
            }
            current = parent;
        }
        crate::util::Point { x, y }
    }

    /// CSSOM View's `clientWidth`: the width of the padding box (border box minus
    /// borders and scrollbar)
    pub fn client_width(&self) -> f32 {
        let layout = self.final_layout();
        layout.size.width - layout.border.left - layout.border.right - layout.scrollbar_size.width
    }

    /// CSSOM View's `clientHeight`: the height of the padding box (border box minus
    /// borders and scrollbar)
    pub fn client_height(&self) -> f32 {
        let layout = self.final_layout();
        layout.size.height - layout.border.top - layout.border.bottom - layout.scrollbar_size.height
    }

    /// CSSOM View's `scrollWidth`: the width of the node's content, including
    /// content not visible due to overflow
    pub fn scroll_width(&self) -> f32 {
        self.client_width()
            .max(self.final_layout().scrollable_overflow_rect.right)
    }

    /// CSSOM View's `scrollHeight`: the height of the node's content, including
    /// content not visible due to overflow
    pub fn scroll_height(&self) -> f32 {
        self.client_height()
            .max(self.final_layout().scrollable_overflow_rect.bottom)
    }

    /// Does the node generate any boxes? (e.g. `getClientRects()` returns an empty
    /// list for boxless nodes, such as `display: none`, `display: contents`, or
    /// detached elements)
    pub fn has_boxes(&self) -> bool {
        self.flags.is_in_document()
            && !self.display_style().is_some_and(|display| {
                matches!(
                    display.inside(),
                    style::values::specified::box_::DisplayInside::None
                        | style::values::specified::box_::DisplayInside::Contents
                )
            })
    }

    /// Creates a synthetic click event
    pub fn synthetic_click_event(&self, mods: Modifiers) -> DomEventData {
        DomEventData::Click(self.synthetic_click_event_data(mods))
    }

    pub fn synthetic_click_event_data(&self, mods: Modifiers) -> BlitzPointerEvent {
        let absolute_position = self.absolute_position(0.0, 0.0);
        let x = absolute_position.x + (self.final_layout().size.width / 2.0);
        let y = absolute_position.y + (self.final_layout().size.height / 2.0);

        BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords: PointerCoords {
                page_x: x,
                page_y: y,

                // TODO: should these be different?
                screen_x: x,
                screen_y: y,
                client_x: x,
                client_y: y,
            },
            mods,
            button: Default::default(),
            buttons: Default::default(),
            details: Default::default(),
            element: Default::default(),
            active_pointers: Default::default(),
        }
    }
}

/// It might be wrong to expose this since what does *equality* mean outside the dom?
impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Node {}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // FIXME: update to reflect changes to fields
        f.debug_struct("NodeData")
            .field("parent", &self.parent)
            .field("id", &self.id)
            .field("is_inline_root", &self.flags.is_inline_root())
            .field("children", &self.children)
            .field("layout_children", &self.layout_children.borrow())
            // .field("style", &self.style)
            .field("node", &self.data)
            .field("stylo_element_data", &self.try_stylo_element_data())
            // .field("unrounded_layout", &self.unrounded_layout)
            // .field("final_layout", &self.final_layout)
            .finish()
    }
}

#[cfg(test)]
mod test {
    use style_dom::ElementState;

    use crate::{Attribute, BaseDocument, DocumentConfig, ElementData, NodeData, qual_name};

    #[test]
    fn create_node_with_disabled_attr() {
        let mut document = BaseDocument::new(DocumentConfig::default());
        let node = document.create_node(NodeData::Element(Box::new(ElementData::new(
            qual_name!("button"),
            vec![Attribute {
                name: qual_name!("disabled"),
                value: "".into(),
            }],
        ))));
        let node = document.get_node(node).unwrap();

        assert!(
            node.element_state().contains(ElementState::DISABLED),
            "form node is disabled"
        );
        assert!(
            !node.element_state().contains(ElementState::ENABLED),
            "form node is not enabled"
        );
    }

    #[test]
    fn ignore_disabled_attr_content() {
        let mut document = BaseDocument::new(DocumentConfig::default());
        let node = document.create_node(NodeData::Element(Box::new(ElementData::new(
            qual_name!("button"),
            vec![Attribute {
                name: qual_name!("disabled"),
                value: "false".into(),
            }],
        ))));
        let node = document.get_node(node).unwrap();

        assert!(
            node.element_state().contains(ElementState::DISABLED),
            "form node is disabled"
        );
        assert!(
            !node.element_state().contains(ElementState::ENABLED),
            "form node is not enabled"
        );
    }

    #[test]
    fn create_node_with_ignored_disable() {
        let mut document = BaseDocument::new(DocumentConfig::default());
        let node = document.create_node(NodeData::Element(Box::new(ElementData::new(
            qual_name!("a"),
            vec![Attribute {
                name: qual_name!("disabled"),
                value: "".into(),
            }],
        ))));
        let node = document.get_node(node).unwrap();

        assert!(
            !node.element_state().contains(ElementState::DISABLED),
            "Non form node cannot be disabled"
        );
        assert!(
            !node.element_state().contains(ElementState::ENABLED),
            "Non form node cannot be enabled"
        );
    }

    #[test]
    fn create_empty_enabled_node() {
        let mut document = BaseDocument::new(DocumentConfig::default());
        let node = document.create_node(NodeData::Element(Box::new(ElementData::new(
            qual_name!("button"),
            vec![],
        ))));
        let node = document.get_node(node).unwrap();

        assert!(
            node.element_state().contains(ElementState::ENABLED),
            "Button should be enabled by default"
        );
    }
}
