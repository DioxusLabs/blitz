use crate::convert;
#[cfg(feature = "writing-modes")]
use crate::frame;
use bitflags::bitflags;
use convert::stylo;
use std::ops::Deref;
use style::logical_geometry::WritingMode;
use style::properties::ComputedValues;
use style::values::CustomIdent;
use style::{Atom, OwnedSlice};
#[cfg(feature = "writing-modes")]
use taffy::ResolveOrZero;

#[cfg(feature = "grid")]
use style::values::{
    computed::{GridTemplateAreas, LengthPercentage},
    generics::grid::{TrackListValue, TrackRepeat, TrackSize},
    specified::position::NamedArea,
};

bitflags! {
    /// Flags for style information that Taffy needs but which is not part of the
    /// stylo [`ComputedValues`]
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
    pub struct StyleFlags: u8 {
        /// Whether the node is a replaced element (e.g. an image or form control)
        const IS_REPLACED = 1 << 0;
    }
}

/// A wrapper struct for anything that `Deref`s to a [`stylo::ComputedValues`](ComputedValues) (can be pointed to by an `&` reference, [`Arc`](std::sync::Arc),
/// [`Ref`](std::cell::Ref), etc). It implements [`taffy`]'s [layout traits](taffy::traits) and can used with Taffy's [layout algorithms](taffy::compute).
pub struct TaffyStyloStyle<T: Deref<Target = ComputedValues>> {
    /// The stylo style
    pub style: T,
    /// Extra node-derived flags that are not part of the stylo style
    pub flags: StyleFlags,
    /// The writing mode whose inline/block axes the Taffy getters are expressed in
    /// (see [`crate::frame`]). Defaults to the style's own `writing-mode`.
    #[cfg(feature = "writing-modes")]
    pub frame: WritingMode,
    /// When set, percentages in `padding` resolve against this length instead of being passed to
    /// Taffy. Used for a box in an orthogonal flow, whose padding percentages resolve against its
    /// containing block's inline size while Taffy would resolve them against the (transposed)
    /// `parent_size.width`, i.e. the containing block's block size.
    #[cfg(feature = "writing-modes")]
    pub percent_basis: Option<f32>,
}

impl<T: Deref<Target = ComputedValues>> TaffyStyloStyle<T> {
    /// Create a new [`TaffyStyloStyle`] from a stylo style and [`StyleFlags`], expressed in the
    /// style's own writing mode
    pub fn new(style: T, flags: StyleFlags) -> Self {
        #[cfg(feature = "writing-modes")]
        let frame = style.writing_mode;
        Self {
            style,
            flags,
            #[cfg(feature = "writing-modes")]
            frame,
            #[cfg(feature = "writing-modes")]
            percent_basis: None,
        }
    }

    /// Create a new [`TaffyStyloStyle`] expressed in the axes of the writing mode `frame`
    /// (the writing mode of the box laying this one out). Without the `writing-modes` feature the
    /// frame is ignored and the getters return physical values.
    #[cfg_attr(not(feature = "writing-modes"), allow(unused_variables))]
    pub fn new_in_frame(style: T, flags: StyleFlags, frame: WritingMode) -> Self {
        Self {
            style,
            flags,
            #[cfg(feature = "writing-modes")]
            frame,
            #[cfg(feature = "writing-modes")]
            percent_basis: None,
        }
    }

    /// Whether the getters swap the physical axes (vertical frame)
    #[inline(always)]
    fn is_vertical(&self) -> bool {
        #[cfg(feature = "writing-modes")]
        {
            self.frame.is_vertical()
        }
        #[cfg(not(feature = "writing-modes"))]
        {
            false
        }
    }

    /// Map a physical rect into the frame
    #[inline(always)]
    fn rect<U>(&self, physical: taffy::Rect<U>) -> taffy::Rect<U> {
        #[cfg(feature = "writing-modes")]
        {
            frame::rect(self.frame, physical)
        }
        #[cfg(not(feature = "writing-modes"))]
        {
            physical
        }
    }

    /// Map a physical size into the frame
    #[inline(always)]
    fn size<U>(&self, physical: taffy::Size<U>) -> taffy::Size<U> {
        if self.is_vertical() {
            physical.transpose()
        } else {
            physical
        }
    }
}

// Deref<stylo::ComputedValues> impl
impl<T: Deref<Target = ComputedValues>> From<T> for TaffyStyloStyle<T> {
    fn from(value: T) -> Self {
        Self::new(value, StyleFlags::empty())
    }
}

// Into<taffy::Style> impl
impl<T: Deref<Target = ComputedValues>> From<TaffyStyloStyle<T>> for taffy::Style<Atom> {
    fn from(value: TaffyStyloStyle<T>) -> Self {
        let mut style = convert::to_taffy_style(&value.style);
        #[cfg(feature = "writing-modes")]
        frame::transpose_style(value.frame, &mut style);
        style.item_is_replaced = value.flags.contains(StyleFlags::IS_REPLACED);
        style
    }
}

// CoreStyle impl
impl<T: Deref<Target = ComputedValues>> taffy::CoreStyle for TaffyStyloStyle<T> {
    type CustomIdent = Atom;

    #[inline]
    fn box_generation_mode(&self) -> taffy::BoxGenerationMode {
        convert::box_generation_mode(self.style.get_box().display)
    }

    #[inline]
    fn is_block(&self) -> bool {
        convert::is_block(self.style.get_box().display)
    }

    #[inline]
    fn is_compressible_replaced(&self) -> bool {
        self.flags.contains(StyleFlags::IS_REPLACED)
    }

    #[inline]
    fn is_replaced(&self) -> bool {
        self.flags.contains(StyleFlags::IS_REPLACED)
    }

    #[inline]
    fn box_sizing(&self) -> taffy::BoxSizing {
        convert::box_sizing(self.style.get_position().box_sizing)
    }

    #[inline]
    fn direction(&self) -> taffy::Direction {
        #[cfg(feature = "writing-modes")]
        {
            // The node's own inline-start side, expressed in Taffy's inline axis
            frame::direction(self.style.writing_mode)
        }
        #[cfg(not(feature = "writing-modes"))]
        {
            convert::direction(self.style.get_inherited_box().direction)
        }
    }

    #[inline]
    fn overflow(&self) -> taffy::Point<taffy::Overflow> {
        let box_styles = self.style.get_box();
        let overflow = taffy::Point {
            x: convert::overflow(box_styles.overflow_x),
            y: convert::overflow(box_styles.overflow_y),
        };
        if self.is_vertical() {
            overflow.transpose()
        } else {
            overflow
        }
    }

    #[inline]
    fn scrollbar_width(&self) -> f32 {
        0.0
    }

    #[inline]
    fn contain(&self) -> taffy::Contain {
        let box_styles = self.style.get_box();
        convert::contain(box_styles.contain, box_styles.display)
    }

    #[inline]
    fn position(&self) -> taffy::Position {
        convert::position(self.style.get_box().position)
    }

    #[inline]
    fn is_containing_block(&self) -> taffy::ContainingBlockClaims {
        convert::containing_block_claims(&self.style)
    }

    #[inline]
    fn inset(&self) -> taffy::Rect<taffy::LengthPercentageAuto> {
        self.rect(convert::inset_rect(&self.style))
    }

    #[inline]
    fn size(&self) -> taffy::Size<taffy::Dimension> {
        let position_styles = self.style.get_position();
        self.size(taffy::Size {
            width: convert::dimension(&position_styles.width),
            height: convert::dimension(&position_styles.height),
        })
    }

    #[inline]
    fn min_size(&self) -> taffy::Size<taffy::LengthPercentageAuto> {
        let position_styles = self.style.get_position();
        self.size(taffy::Size {
            width: convert::min_size(&position_styles.min_width),
            height: convert::min_size(&position_styles.min_height),
        })
    }

    #[inline]
    fn max_size(&self) -> taffy::Size<taffy::LengthPercentageAuto> {
        let position_styles = self.style.get_position();
        self.size(taffy::Size {
            width: convert::max_size(&position_styles.max_width),
            height: convert::max_size(&position_styles.max_height),
        })
    }

    #[inline]
    fn aspect_ratio(&self) -> Option<f32> {
        let ratio = convert::aspect_ratio(self.style.get_position().aspect_ratio);
        if self.is_vertical() {
            ratio.map(|ratio| 1.0 / ratio)
        } else {
            ratio
        }
    }

    #[inline]
    fn margin(&self) -> taffy::Rect<taffy::LengthPercentageAuto> {
        let margin_styles = self.style.get_margin();
        self.rect(taffy::Rect {
            left: convert::margin(&margin_styles.margin_left),
            right: convert::margin(&margin_styles.margin_right),
            top: convert::margin(&margin_styles.margin_top),
            bottom: convert::margin(&margin_styles.margin_bottom),
        })
    }

    #[inline]
    fn padding(&self) -> taffy::Rect<taffy::LengthPercentage> {
        let padding_styles = self.style.get_padding();
        #[cfg(feature = "writing-modes")]
        if let Some(basis) = self.percent_basis {
            let resolve = |value: &stylo::LengthPercentage| {
                taffy::LengthPercentage::length(
                    convert::length_percentage(value)
                        .resolve_or_zero(Some(basis), convert::resolve_calc_value),
                )
            };
            return self.rect(taffy::Rect {
                left: resolve(&padding_styles.padding_left.0),
                right: resolve(&padding_styles.padding_right.0),
                top: resolve(&padding_styles.padding_top.0),
                bottom: resolve(&padding_styles.padding_bottom.0),
            });
        }
        self.rect(taffy::Rect {
            left: convert::length_percentage(&padding_styles.padding_left.0),
            right: convert::length_percentage(&padding_styles.padding_right.0),
            top: convert::length_percentage(&padding_styles.padding_top.0),
            bottom: convert::length_percentage(&padding_styles.padding_bottom.0),
        })
    }

    #[inline]
    fn border(&self) -> taffy::Rect<taffy::LengthPercentage> {
        let border_styles = self.style.get_border();
        self.rect(taffy::Rect {
            left: convert::border(
                &border_styles.border_left_width,
                border_styles.border_left_style,
            ),
            right: convert::border(
                &border_styles.border_right_width,
                border_styles.border_right_style,
            ),
            top: convert::border(
                &border_styles.border_top_width,
                border_styles.border_top_style,
            ),
            bottom: convert::border(
                &border_styles.border_bottom_width,
                border_styles.border_bottom_style,
            ),
        })
    }
}

// BlockContainerStyle impl
#[cfg(feature = "block")]
impl<T: Deref<Target = ComputedValues>> taffy::BlockContainerStyle for TaffyStyloStyle<T> {
    #[inline]
    fn text_align(&self) -> taffy::TextAlign {
        let align = convert::text_align(self.style.clone_text_align());
        #[cfg(feature = "writing-modes")]
        let align = frame::text_align(self.frame, align);
        align
    }

    #[inline]
    fn align_content(&self) -> taffy::AlignContent {
        let display = self.style.clone_display();
        let align_content =
            convert::content_alignment(self.style.get_position().align_content, display);
        if align_content.keyword() == taffy::AlignContentKeyword::Normal
            && display.inside() == stylo::DisplayInside::TableCell
        {
            convert::table_cell_vertical_align(&self.style).unwrap_or(align_content)
        } else {
            align_content
        }
    }

    #[inline]
    fn justify_items(&self) -> taffy::AlignItems {
        convert::default_item_alignment(
            (self.style.get_position().justify_items.computed.0).0,
            self.style.clone_direction() == stylo::Direction::Rtl,
        )
    }
}

// BlockItemStyle impl
#[cfg(feature = "block")]
impl<T: Deref<Target = ComputedValues>> taffy::BlockItemStyle for TaffyStyloStyle<T> {
    #[inline]
    fn is_table(&self) -> bool {
        convert::is_table(self.style.clone_display())
    }

    #[inline]
    fn align_self(&self) -> Option<taffy::AlignSelf> {
        convert::oof_item_alignment(self.style.get_position().align_self.0, false, false)
    }

    #[inline]
    fn justify_self(&self) -> Option<taffy::AlignSelf> {
        convert::oof_item_alignment(
            self.style.get_position().justify_self.0,
            true,
            self.style.clone_direction() == stylo::Direction::Rtl,
        )
    }

    #[inline]
    fn align_content(&self) -> taffy::AlignContent {
        taffy::BlockContainerStyle::align_content(self)
    }

    #[cfg(feature = "floats")]
    #[inline]
    fn float(&self) -> taffy::Float {
        let float = convert::float(self.style.clone_float());
        #[cfg(feature = "writing-modes")]
        let float = frame::float(self.frame, float);
        float
    }

    #[cfg(feature = "floats")]
    #[inline]
    fn clear(&self) -> taffy::Clear {
        let clear = convert::clear(self.style.clone_clear());
        #[cfg(feature = "writing-modes")]
        let clear = frame::clear(self.frame, clear);
        clear
    }
}

// FlexboxContainerStyle impl
#[cfg(feature = "flexbox")]
impl<T: Deref<Target = ComputedValues>> taffy::FlexboxContainerStyle for TaffyStyloStyle<T> {
    #[inline]
    fn flex_direction(&self) -> taffy::FlexDirection {
        let dir = convert::flex_direction(self.style.get_position().flex_direction);
        #[cfg(feature = "writing-modes")]
        let dir = frame::flex_direction(self.frame, dir);
        dir
    }

    #[inline]
    fn flex_wrap(&self) -> taffy::FlexWrap {
        convert::flex_wrap(self.style.get_position().flex_wrap)
    }

    #[inline]
    fn flex_line_count(&self) -> u16 {
        convert::flex_line_count(self.style.get_position().flex_line_count)
    }

    #[inline]
    fn gap(&self) -> taffy::Size<taffy::LengthPercentage> {
        let position_styles = self.style.get_position();
        self.size(taffy::Size {
            width: convert::gap(&position_styles.column_gap),
            height: convert::gap(&position_styles.row_gap),
        })
    }

    #[inline]
    fn align_content(&self) -> taffy::AlignContent {
        convert::content_alignment(
            self.style.get_position().align_content,
            self.style.clone_display(),
        )
    }

    #[inline]
    fn align_items(&self) -> taffy::AlignItems {
        convert::default_item_alignment(self.style.get_position().align_items.0, false)
    }

    #[inline]
    fn justify_content(&self) -> taffy::JustifyContent {
        let position_styles = self.style.get_position();
        convert::justify_content(
            position_styles.justify_content,
            position_styles.flex_direction,
            self.style.clone_direction(),
            self.style.clone_display(),
        )
    }
}

// FlexboxItemStyle impl
#[cfg(feature = "flexbox")]
impl<T: Deref<Target = ComputedValues>> taffy::FlexboxItemStyle for TaffyStyloStyle<T> {
    #[inline]
    fn flex_basis(&self) -> taffy::Dimension {
        convert::flex_basis(&self.style.get_position().flex_basis)
    }

    #[inline]
    fn flex_grow(&self) -> f32 {
        self.style.get_position().flex_grow.0
    }

    #[inline]
    fn flex_shrink(&self) -> f32 {
        self.style.get_position().flex_shrink.0
    }

    #[inline]
    fn align_self(&self) -> Option<taffy::AlignSelf> {
        convert::item_alignment(self.style.get_position().align_self.0, false)
    }
}

#[cfg(feature = "grid")]
pub struct GridAreaWrapper<'a>(pub &'a [NamedArea]);
#[cfg(feature = "grid")]
impl<'a> IntoIterator for GridAreaWrapper<'a> {
    type Item = taffy::GridTemplateArea<Atom>;

    type IntoIter = std::iter::Map<
        std::slice::Iter<'a, NamedArea>,
        for<'b> fn(&'b NamedArea) -> taffy::GridTemplateArea<Atom>,
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter().map(convert::grid_template_area)
    }
}

#[cfg(feature = "grid")]
type SliceMapIter<'a, Input, Output> =
    core::iter::Map<core::slice::Iter<'a, Input>, for<'c> fn(&'c Input) -> Output>;
#[cfg(feature = "grid")]
type SliceMapRefIter<'a, Input, Output> =
    core::iter::Map<core::slice::Iter<'a, Input>, for<'c> fn(&'c Input) -> &'c Output>;

// Line name iterator type aliases
#[cfg(feature = "grid")]
type LineNameSetIter<'a> = SliceMapRefIter<'a, CustomIdent, Atom>;
#[cfg(feature = "grid")]
type LineNameIter<'a> = core::iter::Map<
    core::slice::Iter<'a, OwnedSlice<CustomIdent>>,
    fn(&OwnedSlice<CustomIdent>) -> LineNameSetIter<'_>,
>;

#[derive(Clone)]
#[cfg(feature = "grid")]
pub struct StyloLineNameIter<'a>(LineNameIter<'a>);
#[cfg(feature = "grid")]
impl<'a> StyloLineNameIter<'a> {
    /// Create a new StyloLineNameIter
    pub fn new(names: &'a OwnedSlice<OwnedSlice<CustomIdent>>) -> Self {
        Self(names.iter().map(|names| names.iter().map(|ident| &ident.0)))
    }
}
#[cfg(feature = "grid")]
impl<'a> Iterator for StyloLineNameIter<'a> {
    type Item = core::iter::Map<core::slice::Iter<'a, CustomIdent>, fn(&CustomIdent) -> &Atom>;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}
#[cfg(feature = "grid")]
impl ExactSizeIterator for StyloLineNameIter<'_> {}
#[cfg(feature = "grid")]
impl<'a> taffy::TemplateLineNames<'a, Atom> for StyloLineNameIter<'a> {
    type LineNameSet<'b>
        = SliceMapRefIter<'b, CustomIdent, Atom>
    where
        Self: 'b;
}
#[cfg(feature = "grid")]
pub struct RepetitionWrapper<'a>(&'a TrackRepeat<LengthPercentage, i32>);
#[cfg(feature = "grid")]
impl taffy::GenericRepetition for RepetitionWrapper<'_> {
    type CustomIdent = Atom;

    type RepetitionTrackList<'a>
        = SliceMapIter<'a, stylo::TrackSize<LengthPercentage>, taffy::TrackSizingFunction>
    where
        Self: 'a;

    type TemplateLineNames<'a>
        = StyloLineNameIter<'a>
    where
        Self: 'a;

    fn count(&self) -> taffy::RepetitionCount {
        convert::track_repeat(self.0.count)
    }

    fn tracks(&self) -> Self::RepetitionTrackList<'_> {
        self.0.track_sizes.iter().map(convert::track_size)
    }

    fn lines_names(&self) -> Self::TemplateLineNames<'_> {
        StyloLineNameIter::new(&self.0.line_names)
    }
}

/// Physical grid style accessors. The `*_source` helpers pick the physical property that plays the
/// given Taffy role in the frame (rows and columns swap in a vertical frame).
#[cfg(feature = "grid")]
impl<T: Deref<Target = ComputedValues>> TaffyStyloStyle<T> {
    #[inline]
    fn template_rows_source(&self) -> &stylo::GenericGridTemplateComponent<LengthPercentage, i32> {
        let position_styles = self.style.get_position();
        if self.is_vertical() {
            &position_styles.grid_template_columns
        } else {
            &position_styles.grid_template_rows
        }
    }

    #[inline]
    fn template_columns_source(
        &self,
    ) -> &stylo::GenericGridTemplateComponent<LengthPercentage, i32> {
        let position_styles = self.style.get_position();
        if self.is_vertical() {
            &position_styles.grid_template_rows
        } else {
            &position_styles.grid_template_columns
        }
    }

    #[inline]
    fn auto_rows_source(&self) -> &stylo::ImplicitGridTracks {
        let position_styles = self.style.get_position();
        if self.is_vertical() {
            &position_styles.grid_auto_columns
        } else {
            &position_styles.grid_auto_rows
        }
    }

    #[inline]
    fn auto_columns_source(&self) -> &stylo::ImplicitGridTracks {
        let position_styles = self.style.get_position();
        if self.is_vertical() {
            &position_styles.grid_auto_rows
        } else {
            &position_styles.grid_auto_columns
        }
    }

    fn physical_area_row_count(&self) -> u16 {
        match &self.style.get_position().grid_template_areas {
            GridTemplateAreas::Areas(areas) => areas.0.strings.len() as u16,
            GridTemplateAreas::None => 0,
        }
    }

    fn physical_area_column_count(&self) -> u16 {
        match &self.style.get_position().grid_template_areas {
            GridTemplateAreas::Areas(areas) => areas.0.width as u16,
            GridTemplateAreas::None => 0,
        }
    }

    #[inline]
    fn physical_grid_row(&self) -> taffy::Line<taffy::GridPlacement<Atom>> {
        let position_styles = self.style.get_position();
        taffy::Line {
            start: convert::grid_line(&position_styles.grid_row_start),
            end: convert::grid_line(&position_styles.grid_row_end),
        }
    }

    #[inline]
    fn physical_grid_column(&self) -> taffy::Line<taffy::GridPlacement<Atom>> {
        let position_styles = self.style.get_position();
        taffy::Line {
            start: convert::grid_line(&position_styles.grid_column_start),
            end: convert::grid_line(&position_styles.grid_column_end),
        }
    }
}

#[cfg(feature = "grid")]
impl<T: Deref<Target = ComputedValues>> taffy::GridContainerStyle for TaffyStyloStyle<T> {
    type Repetition<'a>
        = RepetitionWrapper<'a>
    where
        Self: 'a;

    type TemplateTrackList<'a>
        = core::iter::Map<
        core::slice::Iter<'a, TrackListValue<LengthPercentage, i32>>,
        fn(
            &'a TrackListValue<LengthPercentage, i32>,
        ) -> taffy::GenericGridTemplateComponent<Atom, RepetitionWrapper<'a>>,
    >
    where
        Self: 'a;

    type AutoTrackList<'a>
        = SliceMapIter<'a, TrackSize<LengthPercentage>, taffy::TrackSizingFunction>
    where
        Self: 'a;

    type TemplateLineNames<'a>
        = StyloLineNameIter<'a>
    where
        Self: 'a;
    type GridTemplateAreas<'a>
        = SliceMapIter<'a, NamedArea, taffy::GridTemplateArea<Atom>>
    where
        Self: 'a;

    #[inline]
    fn grid_template_rows(&self) -> Option<Self::TemplateTrackList<'_>> {
        match self.template_rows_source() {
            stylo::GenericGridTemplateComponent::None => None,
            stylo::GenericGridTemplateComponent::TrackList(list) => {
                Some(list.values.iter().map(|track| match track {
                    stylo::TrackListValue::TrackSize(size) => {
                        taffy::GenericGridTemplateComponent::Single(convert::track_size(size))
                    }
                    stylo::TrackListValue::TrackRepeat(repeat) => {
                        taffy::GenericGridTemplateComponent::Repeat(RepetitionWrapper(repeat))
                    }
                }))
            }

            // TODO: Implement subgrid and masonry
            stylo::GenericGridTemplateComponent::Subgrid(_) => None,
            stylo::GenericGridTemplateComponent::Masonry => None,
        }
    }

    #[inline]
    fn grid_template_columns(&self) -> Option<Self::TemplateTrackList<'_>> {
        match self.template_columns_source() {
            stylo::GenericGridTemplateComponent::None => None,
            stylo::GenericGridTemplateComponent::TrackList(list) => {
                Some(list.values.iter().map(|track| match track {
                    stylo::TrackListValue::TrackSize(size) => {
                        taffy::GenericGridTemplateComponent::Single(convert::track_size(size))
                    }
                    stylo::TrackListValue::TrackRepeat(repeat) => {
                        taffy::GenericGridTemplateComponent::Repeat(RepetitionWrapper(repeat))
                    }
                }))
            }

            // TODO: Implement subgrid and masonry
            stylo::GenericGridTemplateComponent::Subgrid(_) => None,
            stylo::GenericGridTemplateComponent::Masonry => None,
        }
    }

    #[inline]
    fn grid_auto_rows(&self) -> Self::AutoTrackList<'_> {
        self.auto_rows_source().0.iter().map(convert::track_size)
    }

    #[inline]
    fn grid_auto_columns(&self) -> Self::AutoTrackList<'_> {
        self.auto_columns_source().0.iter().map(convert::track_size)
    }

    fn grid_template_areas(&self) -> Option<Self::GridTemplateAreas<'_>> {
        match &self.style.get_position().grid_template_areas {
            GridTemplateAreas::Areas(areas) => {
                Some(areas.0.areas.iter().map(convert::grid_template_area))
            }
            GridTemplateAreas::None => None,
        }
    }

    fn grid_template_area_row_count(&self) -> u16 {
        match &self.style.get_position().grid_template_areas {
            GridTemplateAreas::Areas(areas) => convert::saturating_u16(areas.0.strings.len()),
            GridTemplateAreas::None => 0,
        }
    }

    fn grid_template_area_column_count(&self) -> u16 {
        match &self.style.get_position().grid_template_areas {
            GridTemplateAreas::Areas(areas) => convert::saturating_u16(areas.0.width),
            GridTemplateAreas::None => 0,
        }
    }

    fn grid_template_column_names(&self) -> Option<Self::TemplateLineNames<'_>> {
        match self.template_columns_source() {
            stylo::GenericGridTemplateComponent::None => None,
            stylo::GenericGridTemplateComponent::TrackList(list) => {
                Some(StyloLineNameIter::new(&list.line_names))
            }
            // TODO: Implement subgrid and masonry
            stylo::GenericGridTemplateComponent::Subgrid(_) => None,
            stylo::GenericGridTemplateComponent::Masonry => None,
        }
    }

    fn grid_template_row_names(&self) -> Option<Self::TemplateLineNames<'_>> {
        match self.template_rows_source() {
            stylo::GenericGridTemplateComponent::None => None,
            stylo::GenericGridTemplateComponent::TrackList(list) => {
                Some(StyloLineNameIter::new(&list.line_names))
            }
            // TODO: Implement subgrid and masonry
            stylo::GenericGridTemplateComponent::Subgrid(_) => None,
            stylo::GenericGridTemplateComponent::Masonry => None,
        }
    }

    #[inline]
    fn grid_auto_flow(&self) -> taffy::GridAutoFlow {
        let flow = convert::grid_auto_flow(self.style.get_position().grid_auto_flow);
        #[cfg(feature = "writing-modes")]
        let flow = frame::grid_auto_flow(self.frame, flow);
        flow
    }

    #[inline]
    fn gap(&self) -> taffy::Size<taffy::LengthPercentage> {
        let position_styles = self.style.get_position();
        self.size(taffy::Size {
            width: convert::gap(&position_styles.column_gap),
            height: convert::gap(&position_styles.row_gap),
        })
    }

    #[inline]
    fn align_content(&self) -> taffy::AlignContent {
        convert::content_alignment(
            self.style.get_position().align_content,
            self.style.clone_display(),
        )
    }

    #[inline]
    fn justify_content(&self) -> taffy::JustifyContent {
        let position_styles = self.style.get_position();
        convert::justify_content(
            position_styles.justify_content,
            position_styles.flex_direction,
            self.style.clone_direction(),
            self.style.clone_display(),
        )
    }

    #[inline]
    fn align_items(&self) -> taffy::AlignItems {
        convert::default_item_alignment(self.style.get_position().align_items.0, false)
    }

    #[inline]
    fn justify_items(&self) -> taffy::AlignItems {
        convert::default_item_alignment(
            (self.style.get_position().justify_items.computed.0).0,
            self.style.clone_direction() == stylo::Direction::Rtl,
        )
    }
}

// GridItemStyle impl
#[cfg(feature = "grid")]
impl<T: Deref<Target = ComputedValues>> taffy::GridItemStyle for TaffyStyloStyle<T> {
    #[inline]
    fn grid_row(&self) -> taffy::Line<taffy::GridPlacement<Atom>> {
        if self.is_vertical() {
            self.physical_grid_column()
        } else {
            self.physical_grid_row()
        }
    }

    #[inline]
    fn grid_column(&self) -> taffy::Line<taffy::GridPlacement<Atom>> {
        if self.is_vertical() {
            self.physical_grid_row()
        } else {
            self.physical_grid_column()
        }
    }

    #[inline]
    fn align_self(&self) -> Option<taffy::AlignSelf> {
        convert::item_alignment(self.style.get_position().align_self.0, false)
    }

    #[inline]
    fn justify_self(&self) -> Option<taffy::AlignSelf> {
        convert::item_alignment(
            self.style.get_position().justify_self.0,
            self.style.clone_direction() == stylo::Direction::Rtl,
        )
    }
}

impl<T: Deref<Target = ComputedValues>> taffy::OofItemStyle for TaffyStyloStyle<T> {
    #[inline]
    fn align_self(&self) -> Option<taffy::AlignSelf> {
        convert::oof_item_alignment(self.style.get_position().align_self.0, false, false)
    }

    #[inline]
    fn justify_self(&self) -> Option<taffy::AlignSelf> {
        convert::oof_item_alignment(
            self.style.get_position().justify_self.0,
            true,
            self.style.clone_direction() == stylo::Direction::Rtl,
        )
    }

    #[inline]
    fn is_table(&self) -> bool {
        convert::is_table(self.style.clone_display())
    }

    #[inline]
    fn grid_row(&self) -> taffy::Line<taffy::GridPlacement<Atom>> {
        if self.is_vertical() {
            self.physical_grid_column()
        } else {
            self.physical_grid_row()
        }
    }

    #[inline]
    fn grid_column(&self) -> taffy::Line<taffy::GridPlacement<Atom>> {
        if self.is_vertical() {
            self.physical_grid_row()
        } else {
            self.physical_grid_column()
        }
    }
}
