//! Style application from CSS computed values to UIKit properties
//!
//! This module translates CSS computed styles (from Stylo) and Taffy layout
//! to UIKit view properties.

use blitz_dom::Node;
use objc2_foundation::{NSPoint, NSRect, NSSize};
use objc2_ui_kit::{UIColor, UIFont, UILabel, UIView};
use style::properties::ComputedValues;

/// Apply layout (position and size) from Taffy to a UIView.
///
/// # Arguments
///
/// * `view` - The UIView to update
/// * `node` - The DOM node with layout information
/// * `parent_origin` - The origin of the parent view in screen coordinates
/// * `scale` - Scale factor (points per CSS pixel)
pub fn apply_layout(view: &UIView, node: &Node, parent_origin: NSPoint, scale: f64) {
    let layout = node.final_layout;

    // Convert Taffy layout (in CSS pixels) to UIKit frame (in points)
    let frame = NSRect::new(
        NSPoint::new(
            parent_origin.x + layout.location.x as f64 * scale,
            parent_origin.y + layout.location.y as f64 * scale,
        ),
        NSSize::new(
            layout.size.width as f64 * scale,
            layout.size.height as f64 * scale,
        ),
    );

    unsafe { view.setFrame(frame) };
}

/// Apply visual styles (background, border, opacity) to a UIView.
pub fn apply_visual_styles(view: &UIView, node: &Node, scale: f64) {
    let Some(styles) = node.primary_styles() else {
        return;
    };

    // Apply background color
    apply_background_color(view, &styles);

    // Apply border
    apply_border(view, &styles, scale);

    // Apply opacity
    apply_opacity(view, &styles);

    // Apply visibility
    apply_visibility(view, &styles);
}

/// Apply text styles to a UILabel.
pub fn apply_text_styles(label: &UILabel, node: &Node, scale: f64) {
    let Some(styles) = node.primary_styles() else {
        return;
    };

    // Apply text color
    apply_text_color(label, &styles);

    // Apply font
    apply_font(label, &styles, scale);

    // Apply text alignment
    apply_text_alignment(label, &styles);
}

// =============================================================================
// Individual Style Applications
// =============================================================================

fn apply_background_color(view: &UIView, styles: &ComputedValues) {
    let current_color = styles.clone_color();
    let bg = styles.clone_background_color();
    let bg_absolute = bg.resolve_to_absolute(&current_color);

    if let Some(ui_color) = stylo_color_to_uicolor(&bg_absolute) {
        unsafe { view.setBackgroundColor(Some(&ui_color)) };
    }
}

fn apply_border(view: &UIView, styles: &ComputedValues, scale: f64) {
    let layer = unsafe { view.layer() };

    let border = styles.get_border();
    let current_color = styles.clone_color();

    // Border width (use top border as representative)
    // Note: UIKit doesn't support non-uniform border widths
    let border_width = border.border_top_width.to_f64_px() * scale;
    unsafe { layer.setBorderWidth(border_width) };

    // Border color - get CGColor from UIColor
    let border_color = border.border_top_color.resolve_to_absolute(&current_color);
    if let Some(ui_color) = stylo_color_to_uicolor(&border_color) {
        let cg_color = unsafe { ui_color.CGColor() };
        // if let Some(cg_color) = unsafe { ui_color.CGColor() } {
        unsafe { layer.setBorderColor(Some(&cg_color)) };
        // }
    }

    // Corner radius
    // Use the top-left radius as representative (UIKit only supports uniform radius)
    // LengthPercentage has a `px()` method to resolve to pixels (using 0 for percentage basis)
    let radii = &border.border_top_left_radius;
    let radius = radii
        .0
        .width
        .0
        .resolve(style::values::computed::Au(0).into())
        .px() as f64
        * scale;

    unsafe { layer.setCornerRadius(radius) };

    // If we have a border radius, we need to clip to bounds
    if radius > 0.0 {
        unsafe { layer.setMasksToBounds(true) };
    }
}

fn apply_opacity(view: &UIView, styles: &ComputedValues) {
    let opacity = styles.get_effects().clone_opacity();
    unsafe { view.setAlpha(opacity as f64) };
}

fn apply_visibility(view: &UIView, styles: &ComputedValues) {
    use style::computed_values::visibility::T as Visibility;

    let visibility = styles.get_inherited_box().visibility;
    let is_hidden = matches!(visibility, Visibility::Hidden | Visibility::Collapse);
    unsafe { view.setHidden(is_hidden) };
}

fn apply_text_color(label: &UILabel, styles: &ComputedValues) {
    let color = styles.clone_color();
    if let Some(ui_color) = stylo_color_to_uicolor(&color) {
        unsafe { label.setTextColor(Some(&ui_color)) };
    }
}

fn apply_font(label: &UILabel, styles: &ComputedValues, scale: f64) {
    let font_style = styles.get_font();

    // Get font size
    let font_size = font_style.font_size.computed_size.px() as f64 * scale;

    // Get font weight
    let weight = font_weight_to_uifont_weight(&font_style);

    // Create UIFont
    // For now, use system font with the appropriate weight
    let ui_font = unsafe { UIFont::systemFontOfSize_weight(font_size, weight) };
    unsafe { label.setFont(Some(&ui_font)) };
}

fn apply_text_alignment(label: &UILabel, styles: &ComputedValues) {
    use objc2_ui_kit::NSTextAlignment;
    use style::computed_values::text_align::T as TextAlign;

    let text_align = styles.clone_text_align();
    let ns_alignment = match text_align {
        TextAlign::Start | TextAlign::Left => NSTextAlignment::Left,
        TextAlign::End | TextAlign::Right => NSTextAlignment::Right,
        TextAlign::Center => NSTextAlignment::Center,
        TextAlign::Justify => NSTextAlignment::Justified,
        _ => NSTextAlignment::Natural,
    };

    unsafe { label.setTextAlignment(ns_alignment) };
}

// =============================================================================
// Color Conversion
// =============================================================================

/// Convert a Stylo absolute color to a UIColor.
fn stylo_color_to_uicolor(
    color: &style::color::AbsoluteColor,
) -> Option<objc2::rc::Retained<UIColor>> {
    // Get sRGB components
    let [r, g, b, a] = color
        .to_color_space(style::color::ColorSpace::Srgb)
        .raw_components()
        .clone();

    Some(unsafe { UIColor::colorWithRed_green_blue_alpha(r as f64, g as f64, b as f64, a as f64) })
}

/// Convert Stylo font weight to UIKit font weight.
fn font_weight_to_uifont_weight(font_style: &style::properties::style_structs::Font) -> f64 {
    use objc2_ui_kit::{
        UIFontWeightBlack, UIFontWeightBold, UIFontWeightHeavy, UIFontWeightLight,
        UIFontWeightMedium, UIFontWeightRegular, UIFontWeightSemibold, UIFontWeightThin,
        UIFontWeightUltraLight,
    };

    let weight = font_style.font_weight.value();

    unsafe {
        // Map CSS font-weight (100-900) to UIKit font weights
        match weight as i32 {
            100 => UIFontWeightUltraLight,
            200 => UIFontWeightThin,
            300 => UIFontWeightLight,
            400 => UIFontWeightRegular,
            500 => UIFontWeightMedium,
            600 => UIFontWeightSemibold,
            700 => UIFontWeightBold,
            800 => UIFontWeightHeavy,
            900 => UIFontWeightBlack,
            _ => {
                // Interpolate for non-standard weights
                if weight < 400.0 {
                    UIFontWeightLight
                } else if weight < 600.0 {
                    UIFontWeightRegular
                } else {
                    UIFontWeightBold
                }
            }
        }
    }
}
