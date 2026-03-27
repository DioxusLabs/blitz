//! Diagnostic audit for unsupported CSS features.
//!
//! After style resolution, each node's computed styles can be checked against the set of
//! CSS features that Blitz actually supports. Any unsupported property values are logged
//! with the node's context (tag, id, class) so the user knows exactly which element
//! triggered the fallback.

use style::properties::ComputedValues;
use style::values::computed::{Rotate, Overflow};
use style::values::generics::transform::{Scale, Translate};
use style::values::specified::box_::{DisplayInside, DisplayOutside};
use style::values::specified::BorderStyle;

use crate::Node;

/// Audit a node's computed styles and log any unsupported CSS features.
///
/// This is called during `flush_styles_to_layout` where we have access to both
/// the node (for context) and its computed styles.
pub(crate) fn audit_unsupported_css(node: &Node, style: &ComputedValues) {
    let mut warnings: Vec<&str> = Vec::new();

    // ── Display ──────────────────────────────────────────────────────────
    let display = style.clone_display();

    match display.inside() {
        DisplayInside::Table => warnings.push("display: table (mapped to grid)"),
        DisplayInside::TableCell => warnings.push("display: table-cell (mapped to block)"),
        _ => {}
    }

    match display.outside() {
        DisplayOutside::TableCaption => warnings.push("display: table-caption (outer, not fully supported)"),
        DisplayOutside::InternalTable => warnings.push("display: internal-table (outer, not fully supported)"),
        _ => {}
    }

    // ── Position ─────────────────────────────────────────────────────────
    use style::properties::longhands::position::computed_value::T as Position;
    let position = style.clone_position();
    match position {
        Position::Fixed => warnings.push("position: fixed (mapped to absolute)"),
        Position::Sticky => warnings.push("position: sticky (mapped to relative)"),
        _ => {}
    }

    // ── Overflow ─────────────────────────────────────────────────────────
    if style.clone_overflow_x() == Overflow::Auto {
        warnings.push("overflow-x: auto (mapped to scroll)");
    }
    if style.clone_overflow_y() == Overflow::Auto {
        warnings.push("overflow-y: auto (mapped to scroll)");
    }

    // ── Sizing keywords ──────────────────────────────────────────────────
    {
        use style::values::generics::length::{GenericSize, GenericMaxSize};

        let pos = style.get_position();

        macro_rules! check_size {
            ($val:expr, $prop:literal) => {
                match $val {
                    GenericSize::MaxContent => warnings.push(concat!($prop, ": max-content (mapped to auto)")),
                    GenericSize::MinContent => warnings.push(concat!($prop, ": min-content (mapped to auto)")),
                    GenericSize::FitContent => warnings.push(concat!($prop, ": fit-content (mapped to auto)")),
                    GenericSize::FitContentFunction(_) => warnings.push(concat!($prop, ": fit-content() (mapped to auto)")),
                    GenericSize::Stretch => warnings.push(concat!($prop, ": stretch (mapped to auto)")),
                    GenericSize::WebkitFillAvailable => warnings.push(concat!($prop, ": -webkit-fill-available (mapped to auto)")),
                    _ => {}
                }
            };
        }

        macro_rules! check_max_size {
            ($val:expr, $prop:literal) => {
                match $val {
                    GenericMaxSize::MaxContent => warnings.push(concat!($prop, ": max-content (mapped to auto)")),
                    GenericMaxSize::MinContent => warnings.push(concat!($prop, ": min-content (mapped to auto)")),
                    GenericMaxSize::FitContent => warnings.push(concat!($prop, ": fit-content (mapped to auto)")),
                    GenericMaxSize::FitContentFunction(_) => warnings.push(concat!($prop, ": fit-content() (mapped to auto)")),
                    GenericMaxSize::Stretch => warnings.push(concat!($prop, ": stretch (mapped to auto)")),
                    GenericMaxSize::WebkitFillAvailable => warnings.push(concat!($prop, ": -webkit-fill-available (mapped to auto)")),
                    _ => {}
                }
            };
        }

        check_size!(&pos.width, "width");
        check_size!(&pos.height, "height");
        check_size!(&pos.min_width, "min-width");
        check_size!(&pos.min_height, "min-height");
        check_max_size!(&pos.max_width, "max-width");
        check_max_size!(&pos.max_height, "max-height");
    }

    // ── Flex ─────────────────────────────────────────────────────────────
    {
        use style::values::generics::flex::GenericFlexBasis;
        let pos = style.get_position();
        if matches!(&pos.flex_basis, GenericFlexBasis::Content) {
            warnings.push("flex-basis: content (mapped to auto)");
        }
    }

    // ── Grid subgrid / masonry ───────────────────────────────────────────
    {
        use style::values::specified::GenericGridTemplateComponent;
        let pos = style.get_position();
        if matches!(&pos.grid_template_rows, GenericGridTemplateComponent::Subgrid(_)) {
            warnings.push("grid-template-rows: subgrid (not supported)");
        }
        if matches!(&pos.grid_template_rows, GenericGridTemplateComponent::Masonry) {
            warnings.push("grid-template-rows: masonry (not supported)");
        }
        if matches!(&pos.grid_template_columns, GenericGridTemplateComponent::Subgrid(_)) {
            warnings.push("grid-template-columns: subgrid (not supported)");
        }
        if matches!(&pos.grid_template_columns, GenericGridTemplateComponent::Masonry) {
            warnings.push("grid-template-columns: masonry (not supported)");
        }
    }

    // ── Transforms ───────────────────────────────────────────────────────
    {
        let box_styles = style.get_box();

        if matches!(&box_styles.rotate, Rotate::Rotate3D(_, _, _, _)) {
            warnings.push("rotate: 3D rotation (not supported, ignored)");
        }

        // Check for 3D transforms in the transform list
        if !box_styles.transform.0.is_empty() {
            if let Ok((_t, has_3d)) = box_styles
                .transform
                .to_transform_3d_matrix(None)
            {
                if has_3d {
                    warnings.push("transform: 3D transform (not supported, ignored)");
                }
            }
        }

        // Check for 3D translate
        if let Translate::Translate(_x, _y, z) = &box_styles.translate {
            if z.px() != 0.0 {
                warnings.push("translate: 3D z-component (ignored)");
            }
        }

        // Check for 3D scale
        if let Scale::Scale(_x, _y, z) = &box_styles.scale {
            if (*z - 1.0).abs() > f32::EPSILON {
                warnings.push("scale: 3D z-component (ignored)");
            }
        }
    }

    // ── Effects (completely unsupported) ──────────────────────────────────
    {
        let effects = style.get_effects();

        // filter
        if !effects.filter.0.is_empty() {
            warnings.push("filter (not supported)");
        }

        // mix-blend-mode
        use style::computed_values::mix_blend_mode::T as MixBlendMode;
        if effects.mix_blend_mode != MixBlendMode::Normal {
            warnings.push("mix-blend-mode (not supported)");
        }
    }

    // ── SVG / masking / clipping ─────────────────────────────────────────
    {
        let svg = style.get_svg();

        // clip-path
        use style::values::generics::basic_shape::ClipPath;
        if !matches!(&svg.clip_path, ClipPath::None) {
            warnings.push("clip-path (not supported)");
        }
    }

    // ── Border styles (partial support) ──────────────────────────────────
    {
        let border = style.get_border();

        let check_border_style = |bs: BorderStyle| -> Option<&'static str> {
            match bs {
                BorderStyle::None | BorderStyle::Hidden | BorderStyle::Solid => None,
                BorderStyle::Dotted => Some("dotted"),
                BorderStyle::Dashed => Some("dashed"),
                BorderStyle::Double => Some("double"),
                BorderStyle::Groove => Some("groove"),
                BorderStyle::Ridge => Some("ridge"),
                BorderStyle::Inset => Some("inset"),
                BorderStyle::Outset => Some("outset"),
            }
        };

        // Collect unique unsupported border styles
        let mut seen_border_styles: Vec<&str> = Vec::new();
        for bs in [
            border.border_top_style,
            border.border_right_style,
            border.border_bottom_style,
            border.border_left_style,
        ] {
            if let Some(name) = check_border_style(bs) {
                if !seen_border_styles.contains(&name) {
                    seen_border_styles.push(name);
                }
            }
        }
        for name in seen_border_styles {
            match name {
                "dotted" => warnings.push("border-style: dotted (rendered as solid)"),
                "dashed" => warnings.push("border-style: dashed (rendered as solid)"),
                "double" => warnings.push("border-style: double (rendered as solid)"),
                "groove" => warnings.push("border-style: groove (rendered as solid)"),
                "ridge" => warnings.push("border-style: ridge (rendered as solid)"),
                "inset" => warnings.push("border-style: inset (rendered as solid)"),
                "outset" => warnings.push("border-style: outset (rendered as solid)"),
                _ => {}
            }
        }
    }

    // ── Outline style (partial support) ──────────────────────────────────
    {
        use style::values::specified::OutlineStyle;
        let outline = style.get_outline();
        if let OutlineStyle::BorderStyle(bs) = outline.outline_style {
            match bs {
                BorderStyle::Dotted | BorderStyle::Dashed | BorderStyle::Double
                | BorderStyle::Groove | BorderStyle::Ridge | BorderStyle::Inset
                | BorderStyle::Outset => {
                    warnings.push("outline-style (non-solid styles rendered as solid)");
                }
                _ => {}
            }
        }
    }

    // ── Background images (unsupported types) ────────────────────────────
    {
        use style::values::computed::image::Image;
        let bg = style.get_background();
        for img in bg.background_image.0.iter() {
            match img {
                Image::None | Image::Url(_) | Image::Gradient(_) => {}
                Image::LightDark(_) => warnings.push("background-image: light-dark() (not supported)"),
                Image::PaintWorklet(_) => warnings.push("background-image: paint() worklet (not supported)"),
                Image::CrossFade(_) => warnings.push("background-image: cross-fade() (not supported)"),
                Image::ImageSet(_) => warnings.push("background-image: image-set() (not supported)"),
            }
        }
    }

    // ── White-space collapse (partial) ───────────────────────────────────
    {
        use style::computed_values::white_space_collapse::T as WhiteSpaceCollapse;
        let wsc = style.get_inherited_text().white_space_collapse;
        match wsc {
            WhiteSpaceCollapse::PreserveBreaks => {
                warnings.push("white-space-collapse: preserve-breaks (mapped to preserve)");
            }
            WhiteSpaceCollapse::BreakSpaces => {
                warnings.push("white-space-collapse: break-spaces (mapped to preserve)");
            }
            _ => {}
        }
    }

    // ── Emit ─────────────────────────────────────────────────────────────
    if !warnings.is_empty() {
        let node_ctx = node.node_debug_str();
        let joined = warnings.join(", ");
        tracing::warn!(
            node = %node_ctx,
            "Unsupported CSS: {joined}"
        );
    }
}
