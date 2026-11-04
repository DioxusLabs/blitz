//! By-value getters for `Copy` computed values

use style::properties::ComputedValues;
use style::properties::longhands;

macro_rules! copy_getters {
    ($($prop:ident: $clone:ident => $get:ident;)*) => {
        /// By-value getters for longhands whose computed value is `Copy`, so cannot allocate or bump a refcount.
        ///
        /// Stylo's `slow_clone_*` getters may do either; these are the cheap subset.
        pub trait CopyStyleGetters {
            $(
                fn $clone(&self) -> longhands::$prop::computed_value::T;
            )*
            fn clone_position(&self) -> longhands::position::computed_value::T;
        }

        impl CopyStyleGetters for ComputedValues {
            $(
                #[inline(always)]
                fn $clone(&self) -> longhands::$prop::computed_value::T {
                    *self.$get()
                }
            )*
            // `ComputedValues::get_position` is the `Position` style struct getter
            #[inline(always)]
            fn clone_position(&self) -> longhands::position::computed_value::T {
                *self.get_box().get_position()
            }
        }
    };
}

copy_getters! {
    alignment_baseline: clone_alignment_baseline => get_alignment_baseline;
    baseline_source: clone_baseline_source => get_baseline_source;
    border_collapse: clone_border_collapse => get_border_collapse;
    border_spacing: clone_border_spacing => get_border_spacing;
    box_sizing: clone_box_sizing => get_box_sizing;
    clear: clone_clear => get_clear;
    color: clone_color => get_color;
    contain: clone_contain => get_contain;
    direction: clone_direction => get_direction;
    display: clone_display => get_display;
    float: clone_float => get_float;
    image_rendering: clone_image_rendering => get_image_rendering;
    line_height: clone_line_height => get_line_height;
    list_style_position: clone_list_style_position => get_list_style_position;
    object_fit: clone_object_fit => get_object_fit;
    opacity: clone_opacity => get_opacity;
    order: clone_order => get_order;
    overflow_x: clone_overflow_x => get_overflow_x;
    overflow_y: clone_overflow_y => get_overflow_y;
    pointer_events: clone_pointer_events => get_pointer_events;
    scroll_behavior: clone_scroll_behavior => get_scroll_behavior;
    scrollbar_width: clone_scrollbar_width => get_scrollbar_width;
    table_layout: clone_table_layout => get_table_layout;
    text_align: clone_text_align => get_text_align;
    text_align_last: clone_text_align_last => get_text_align_last;
    text_transform: clone_text_transform => get_text_transform;
    text_wrap_mode: clone_text_wrap_mode => get_text_wrap_mode;
    touch_action: clone_touch_action => get_touch_action;
    unicode_bidi: clone_unicode_bidi => get_unicode_bidi;
    user_select: clone_user_select => get_user_select;
    visibility: clone_visibility => get_visibility;
    white_space_collapse: clone_white_space_collapse => get_white_space_collapse;
    z_index: clone_z_index => get_z_index;
}
