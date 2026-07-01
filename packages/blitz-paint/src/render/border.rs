use anyrender::PaintScene;
use blitz_dom::node::SpecialElementData;
use kurbo::{BezPath, Rect};
use peniko::{Color, Fill};
use style::{
    computed_values::border_collapse::T as BorderCollapse,
    values::computed::{BorderStyle, OutlineStyle},
};

use crate::{color::ToColorColor as _, kurbo_css::Edge, render::ElementCx};

impl ElementCx<'_, '_> {
    /// Draw all borders for a node
    pub(crate) fn draw_border(&self, scene: &mut impl PaintScene) {
        let style = &*self.style;
        let border = style.get_border();
        let current_color = style.clone_color();

        let mut borders: [(Color, Option<BezPath>); 4] = [
            (Color::TRANSPARENT, None),
            (Color::TRANSPARENT, None),
            (Color::TRANSPARENT, None),
            (Color::TRANSPARENT, None),
        ];
        let mut count = 0;

        for &edge in &[Edge::Top, Edge::Right, Edge::Bottom, Edge::Left] {
            let color = match edge {
                Edge::Top => &border.border_top_color,
                Edge::Right => &border.border_right_color,
                Edge::Bottom => &border.border_bottom_color,
                Edge::Left => &border.border_left_color,
            }
            .resolve_to_absolute(&current_color)
            .as_srgb_color();

            if color.components[3] > 0.0 {
                borders[count] = (color, Some(self.frame.border_edge_shape(edge)));
                count += 1;
            }
        }

        if count == 0 {
            return;
        }

        // Group together identical colors by sorting.
        let active_slice = &mut borders[0..count];
        active_slice.sort_unstable_by(|a, b| {
            a.0.components
                .partial_cmp(&b.0.components)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut start_border_index = 0;
        while start_border_index < count {
            let color = borders[start_border_index].0;
            let mut next_border_index = start_border_index + 1;
            let has_multiple_edges =
                next_border_index < count && borders[next_border_index].0 == color;
            if has_multiple_edges {
                let mut border_path = borders[start_border_index].1.take().unwrap();
                while next_border_index < count && borders[next_border_index].0 == color {
                    border_path.extend(&borders[next_border_index].1.take().unwrap());
                    next_border_index += 1;
                }
                scene.fill(Fill::NonZero, self.transform, color, None, &border_path);
            } else {
                scene.fill(
                    Fill::NonZero,
                    self.transform,
                    color,
                    None,
                    borders[start_border_index].1.as_ref().unwrap(),
                );
            }
            start_border_index = next_border_index;
        }
    }

    pub(crate) fn draw_table_borders(&self, scene: &mut impl PaintScene) {
        let SpecialElementData::TableRoot(table) = &self.element.special_data else {
            return;
        };
        // Borders are only handled at the table level when BorderCollapse::Collapse
        if table.border_collapse != BorderCollapse::Collapse {
            return;
        }

        let Some(grid_info) = &mut *table.computed_grid_info.borrow_mut() else {
            return;
        };
        let Some(border_style) = table.border_style.as_deref() else {
            return;
        };

        let outer_border_style = self.style.get_border();

        let cols = &grid_info.columns;
        let rows = &grid_info.rows;

        let inner_width =
            (cols.sizes.iter().sum::<f32>() + cols.gutters.iter().sum::<f32>()) as f64;
        let inner_height =
            (rows.sizes.iter().sum::<f32>() + rows.gutters.iter().sum::<f32>()) as f64;

        // TODO: support different colors for different borders
        let current_color = self.style.clone_color();
        let border_color = border_style
            .border_top_color
            .resolve_to_absolute(&current_color)
            .as_srgb_color();

        // No need to draw transparent borders (as they won't be visible anyway)
        if border_color == Color::TRANSPARENT {
            return;
        }

        let border_width = border_style.border_top_width.0.to_f64_px();

        // Draw horizontal inner borders
        let mut y = 0.0;
        for (&height, &gutter) in rows.sizes.iter().zip(rows.gutters.iter()) {
            let shape =
                Rect::new(0.0, y, inner_width, y + gutter as f64).scale_from_origin(self.scale);
            scene.fill(Fill::NonZero, self.transform, border_color, None, &shape);

            y += (height + gutter) as f64;
        }

        // Draw horizontal outer borders
        // Top border
        if outer_border_style.border_top_style != BorderStyle::Hidden {
            let shape =
                Rect::new(0.0, 0.0, inner_width, border_width).scale_from_origin(self.scale);
            scene.fill(Fill::NonZero, self.transform, border_color, None, &shape);
        }
        // Bottom border
        if outer_border_style.border_bottom_style != BorderStyle::Hidden {
            let shape = Rect::new(0.0, inner_height, inner_width, inner_height + border_width)
                .scale_from_origin(self.scale);
            scene.fill(Fill::NonZero, self.transform, border_color, None, &shape);
        }

        // Draw vertical inner borders
        let mut x = 0.0;
        for (&width, &gutter) in cols.sizes.iter().zip(cols.gutters.iter()) {
            let shape =
                Rect::new(x, 0.0, x + gutter as f64, inner_height).scale_from_origin(self.scale);
            scene.fill(Fill::NonZero, self.transform, border_color, None, &shape);

            x += (width + gutter) as f64;
        }

        // Draw vertical outer borders
        // Left border
        if outer_border_style.border_left_style != BorderStyle::Hidden {
            let shape =
                Rect::new(0.0, 0.0, border_width, inner_height).scale_from_origin(self.scale);
            scene.fill(Fill::NonZero, self.transform, border_color, None, &shape);
        }
        // Right border
        if outer_border_style.border_right_style != BorderStyle::Hidden {
            let shape = Rect::new(inner_width, 0.0, inner_width + border_width, inner_height)
                .scale_from_origin(self.scale);
            scene.fill(Fill::NonZero, self.transform, border_color, None, &shape);
        }
    }

    /// ❌ dotted - Defines a dotted border
    /// ❌ dashed - Defines a dashed border
    /// ✅ solid - Defines a solid border
    /// ❌ double - Defines a double border
    /// ❌ groove - Defines a 3D grooved border. The effect depends on the border-color value
    /// ❌ ridge - Defines a 3D ridged border. The effect depends on the border-color value
    /// ❌ inset - Defines a 3D inset border. The effect depends on the border-color value
    /// ❌ outset - Defines a 3D outset border. The effect depends on the border-color value
    /// ✅ none - Defines no border
    /// ✅ hidden - Defines a hidden border
    pub(crate) fn draw_outline(&self, scene: &mut impl PaintScene) {
        let outline = self.style.get_outline();

        let current_color = self.style.clone_color();
        let color = outline
            .outline_color
            .resolve_to_absolute(&current_color)
            .as_srgb_color();

        let style = match outline.outline_style {
            OutlineStyle::Auto => return,
            OutlineStyle::BorderStyle(style) => style,
        };

        let path = match style {
            BorderStyle::None | BorderStyle::Hidden => return,
            BorderStyle::Solid => self.frame.outline(),

            // TODO: Implement other border styles
            BorderStyle::Inset
            | BorderStyle::Groove
            | BorderStyle::Outset
            | BorderStyle::Ridge
            | BorderStyle::Dotted
            | BorderStyle::Dashed
            | BorderStyle::Double => self.frame.outline(),
        };

        scene.fill(Fill::NonZero, self.transform, color, None, &path);
    }
}
