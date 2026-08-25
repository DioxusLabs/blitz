use crate::convert;
use bitflags::bitflags;
use convert::stylo;
use std::ops::Deref;
use style::properties::ComputedValues;
use style::values::CustomIdent;
use style::{Atom, OwnedSlice};

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
}

impl<T: Deref<Target = ComputedValues>> TaffyStyloStyle<T> {
    /// Create a new [`TaffyStyloStyle`] from a stylo style and [`StyleFlags`]
    pub fn new(style: T, flags: StyleFlags) -> Self {
        Self { style, flags }
    }
}

// Deref<stylo::ComputedValues> impl
impl<T: Deref<Target = ComputedValues>> From<T> for TaffyStyloStyle<T> {
    fn from(value: T) -> Self {
        Self {
            style: value,
            flags: StyleFlags::empty(),
        }
    }
}

// Into<taffy::Style> impl
impl<T: Deref<Target = ComputedValues>> From<TaffyStyloStyle<T>> for taffy::Style<Atom> {
    fn from(value: TaffyStyloStyle<T>) -> Self {
        let mut style = convert::to_taffy_style(&value.style);
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
    fn box_sizing(&self) -> taffy::BoxSizing {
        convert::box_sizing(self.style.get_position().box_sizing)
    }

    #[inline]
    fn direction(&self) -> taffy::Direction {
        convert::direction(self.style.get_inherited_box().direction)
    }

    #[inline]
    fn overflow(&self) -> taffy::Point<taffy::Overflow> {
        let box_styles = self.style.get_box();
        taffy::Point {
            x: convert::overflow(box_styles.overflow_x),
            y: convert::overflow(box_styles.overflow_y),
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
    fn inset(&self) -> taffy::Rect<taffy::LengthPercentageAuto> {
        let position_styles = self.style.get_position();
        taffy::Rect {
            left: convert::inset(&position_styles.left),
            right: convert::inset(&position_styles.right),
            top: convert::inset(&position_styles.top),
            bottom: convert::inset(&position_styles.bottom),
        }
    }

    #[inline]
    fn size(&self) -> taffy::Size<taffy::Dimension> {
        let position_styles = self.style.get_position();
        taffy::Size {
            width: convert::dimension(&position_styles.width),
            height: convert::dimension(&position_styles.height),
        }
    }

    #[inline]
    fn min_size(&self) -> taffy::Size<taffy::LengthPercentageAuto> {
        let position_styles = self.style.get_position();
        taffy::Size {
            width: convert::min_size(&position_styles.min_width),
            height: convert::min_size(&position_styles.min_height),
        }
    }

    #[inline]
    fn max_size(&self) -> taffy::Size<taffy::LengthPercentageAuto> {
        let position_styles = self.style.get_position();
        taffy::Size {
            width: convert::max_size(&position_styles.max_width),
            height: convert::max_size(&position_styles.max_height),
        }
    }

    #[inline]
    fn aspect_ratio(&self) -> Option<f32> {
        convert::aspect_ratio(self.style.get_position().aspect_ratio)
    }

    #[inline]
    fn margin(&self) -> taffy::Rect<taffy::LengthPercentageAuto> {
        let margin_styles = self.style.get_margin();
        taffy::Rect {
            left: convert::margin(&margin_styles.margin_left),
            right: convert::margin(&margin_styles.margin_right),
            top: convert::margin(&margin_styles.margin_top),
            bottom: convert::margin(&margin_styles.margin_bottom),
        }
    }

    #[inline]
    fn padding(&self) -> taffy::Rect<taffy::LengthPercentage> {
        let padding_styles = self.style.get_padding();
        taffy::Rect {
            left: convert::length_percentage(&padding_styles.padding_left.0),
            right: convert::length_percentage(&padding_styles.padding_right.0),
            top: convert::length_percentage(&padding_styles.padding_top.0),
            bottom: convert::length_percentage(&padding_styles.padding_bottom.0),
        }
    }

    #[inline]
    fn border(&self) -> taffy::Rect<taffy::LengthPercentage> {
        let border_styles = self.style.get_border();
        taffy::Rect {
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
        }
    }
}

// BlockContainerStyle impl
#[cfg(feature = "block")]
impl<T: Deref<Target = ComputedValues>> taffy::BlockContainerStyle for TaffyStyloStyle<T> {
    #[inline]
    fn text_align(&self) -> taffy::TextAlign {
        convert::text_align(self.style.clone_text_align())
    }

    #[inline]
    fn align_content(&self) -> Option<taffy::AlignContent> {
        convert::content_alignment(
            self.style.get_position().align_content,
            self.style.clone_display(),
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

    #[cfg(feature = "floats")]
    #[inline]
    fn float(&self) -> taffy::Float {
        convert::float(self.style.clone_float())
    }

    #[cfg(feature = "floats")]
    #[inline]
    fn clear(&self) -> taffy::Clear {
        convert::clear(self.style.clone_clear())
    }
}

// FlexboxContainerStyle impl
#[cfg(feature = "flexbox")]
impl<T: Deref<Target = ComputedValues>> taffy::FlexboxContainerStyle for TaffyStyloStyle<T> {
    #[inline]
    fn flex_direction(&self) -> taffy::FlexDirection {
        convert::flex_direction(self.style.get_position().flex_direction)
    }

    #[inline]
    fn flex_wrap(&self) -> taffy::FlexWrap {
        convert::flex_wrap(self.style.get_position().flex_wrap)
    }

    #[inline]
    fn gap(&self) -> taffy::Size<taffy::LengthPercentage> {
        let position_styles = self.style.get_position();
        taffy::Size {
            width: convert::gap(&position_styles.column_gap),
            height: convert::gap(&position_styles.row_gap),
        }
    }

    #[inline]
    fn align_content(&self) -> Option<taffy::AlignContent> {
        convert::content_alignment(
            self.style.get_position().align_content,
            self.style.clone_display(),
        )
    }

    #[inline]
    fn align_items(&self) -> Option<taffy::AlignItems> {
        convert::item_alignment(self.style.get_position().align_items.0, false)
    }

    #[inline]
    fn justify_content(&self) -> Option<taffy::JustifyContent> {
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
        match &self.style.get_position().grid_template_rows {
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
        match &self.style.get_position().grid_template_columns {
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
        self.style
            .get_position()
            .grid_auto_rows
            .0
            .iter()
            .map(convert::track_size)
    }

    #[inline]
    fn grid_auto_columns(&self) -> Self::AutoTrackList<'_> {
        self.style
            .get_position()
            .grid_auto_columns
            .0
            .iter()
            .map(convert::track_size)
    }

    fn grid_template_areas(&self) -> Option<Self::GridTemplateAreas<'_>> {
        match &self.style.get_position().grid_template_areas {
            GridTemplateAreas::Areas(areas) => {
                Some(areas.0.areas.iter().map(|area| taffy::GridTemplateArea {
                    name: area.name.clone(),
                    row_start: area.rows.start as u16,
                    row_end: area.rows.end as u16,
                    column_start: area.columns.start as u16,
                    column_end: area.columns.end as u16,
                }))
            }
            GridTemplateAreas::None => None,
        }
    }

    fn grid_template_area_row_count(&self) -> u16 {
        match &self.style.get_position().grid_template_areas {
            GridTemplateAreas::Areas(areas) => areas.0.strings.len() as u16,
            GridTemplateAreas::None => 0,
        }
    }

    fn grid_template_area_column_count(&self) -> u16 {
        match &self.style.get_position().grid_template_areas {
            GridTemplateAreas::Areas(areas) => areas.0.width as u16,
            GridTemplateAreas::None => 0,
        }
    }

    fn grid_template_column_names(&self) -> Option<Self::TemplateLineNames<'_>> {
        match &self.style.get_position().grid_template_columns {
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
        match &self.style.get_position().grid_template_rows {
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
        convert::grid_auto_flow(self.style.get_position().grid_auto_flow)
    }

    #[inline]
    fn gap(&self) -> taffy::Size<taffy::LengthPercentage> {
        let position_styles = self.style.get_position();
        taffy::Size {
            width: convert::gap(&position_styles.column_gap),
            height: convert::gap(&position_styles.row_gap),
        }
    }

    #[inline]
    fn align_content(&self) -> Option<taffy::AlignContent> {
        convert::content_alignment(
            self.style.get_position().align_content,
            self.style.clone_display(),
        )
    }

    #[inline]
    fn justify_content(&self) -> Option<taffy::JustifyContent> {
        let position_styles = self.style.get_position();
        convert::justify_content(
            position_styles.justify_content,
            position_styles.flex_direction,
            self.style.clone_direction(),
            self.style.clone_display(),
        )
    }

    #[inline]
    fn align_items(&self) -> Option<taffy::AlignItems> {
        convert::item_alignment(self.style.get_position().align_items.0, false)
    }

    #[inline]
    fn justify_items(&self) -> Option<taffy::AlignItems> {
        convert::item_alignment(
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
        let position_styles = self.style.get_position();
        taffy::Line {
            start: convert::grid_line(&position_styles.grid_row_start),
            end: convert::grid_line(&position_styles.grid_row_end),
        }
    }

    #[inline]
    fn grid_column(&self) -> taffy::Line<taffy::GridPlacement<Atom>> {
        let position_styles = self.style.get_position();
        taffy::Line {
            start: convert::grid_line(&position_styles.grid_column_start),
            end: convert::grid_line(&position_styles.grid_column_end),
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
