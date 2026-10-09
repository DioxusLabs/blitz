//! All integration tests are compiled into this single test binary because linking
//! one binary per file dominates build time. New test files must be added here.

mod abspos_inline_span_containing_block;
mod accessibility_hidden;
mod accessibility_roles;
mod animations;
mod anonymous_block_cache_invalidation;
mod anonymous_block_leak;
mod anonymous_block_percentage_height;
mod autofocus_attribute;
mod background_size;
mod br_trailing_line;
mod comment_layout;
mod custom_widget_layout;
mod detached_attribute;
mod details_element;
mod device_coalescing;
mod dir_attribute;
mod display_contents;
mod flex_grid_order;
mod focusability_updates;
mod fragment_navigation;
mod harness_smoke;
mod hover_dom_ancestors;
mod incremental_oracle;
mod inline_bfc_padding;
mod inline_box_baseline;
mod inline_box_scrollable_overflow;
mod inline_fragment_rects;
mod inline_svg_restyle;
mod inline_svg_serialize;
mod inner_html_leak;
mod interaction_state_canonicalization;
mod interaction_state_teardown;
mod lang_attribute;
mod line_break;
mod link_rel_attribute;
mod oof_dynamic_cb;
mod outset_box_shadow_shape;
mod paint_order;
mod paint_tree_bench;
mod paint_tree_incremental;
mod pointer_events;
mod pre_overflow_scroll;
mod pseudo_element_update;
mod rem_after_viewport_change;
mod render_blocking_stylesheet;
mod resize_restyle;
mod rotate_z_axis;
mod scoped_query_selector;
mod scrollbar_drag;
mod scrollbars;
mod stale_dirty_descendants;
mod stale_interaction_state;
mod stale_node_mapping;
mod style_property_invalidation;
mod svg_attr_sizing;
mod svg_background_size;
mod text_selection_anonymous_block;
mod text_transform;
mod touch_action;
mod touch_events;
mod transform_2d_subset;
mod transform_viewport_scale;
mod whitespace_modes;

#[test]
fn all_test_files_are_included() {
    let listed = include_str!("all.rs");
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let name = path.file_stem().unwrap().to_str().unwrap();
        if name != "all" {
            assert!(
                listed.contains(&format!("\nmod {name};")),
                "tests/{name}.rs is not listed in tests/all.rs"
            );
        }
    }
}
