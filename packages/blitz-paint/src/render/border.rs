use anyrender::PaintScene;
use blitz_dom::node::SpecialElementData;
use kurbo::{BezPath, Circle, Point, Rect, Shape as _};
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
            let (color, edge_style) = match edge {
                Edge::Top => (&border.border_top_color, border.border_top_style),
                Edge::Right => (&border.border_right_color, border.border_right_style),
                Edge::Bottom => (&border.border_bottom_color, border.border_bottom_style),
                Edge::Left => (&border.border_left_color, border.border_left_style),
            };
            let color = color.resolve_to_absolute(&current_color).as_srgb_color();

            if color.components[3] <= 0.0 {
                continue;
            }

            match edge_style {
                // `none`/`hidden` produce a zero-width border during layout, but
                // guard against drawing them anyway.
                BorderStyle::None | BorderStyle::Hidden => {}

                // Dashed and dotted edges are drawn immediately as their own
                // (clipped) shapes rather than being batched with the solid edges.
                BorderStyle::Dotted => self.draw_dashed_border_edge(scene, edge, color, true),
                BorderStyle::Dashed => self.draw_dashed_border_edge(scene, edge, color, false),

                // A double border is two solid lines separated by a gap, splitting
                // the border width into three equal parts (outer line / gap / inner
                // line). Both lines share a color, so both rings are placed into a
                // single path and batched together with the other solid edges.
                BorderStyle::Double => {
                    let edge_width = match edge {
                        Edge::Top => self.frame.border_width.y0,
                        Edge::Bottom => self.frame.border_width.y1,
                        Edge::Left => self.frame.border_width.x0,
                        Edge::Right => self.frame.border_width.x1,
                    };

                    // Needs at least 3px (one device pixel per line and per gap) to
                    // render as two lines; thinner borders fall back to a solid
                    // fill, matching browser behaviour.
                    let path = if edge_width < 3.0 * self.scale {
                        self.frame.border_edge_shape(edge)
                    } else {
                        let mut path = self
                            .frame
                            .border_slice(0.0, 1.0 / 3.0)
                            .border_edge_shape(edge);
                        path.extend(
                            &self
                                .frame
                                .border_slice(2.0 / 3.0, 1.0)
                                .border_edge_shape(edge),
                        );
                        path
                    };
                    borders[count] = (color, Some(path));
                    count += 1;
                }

                // Solid (and, for now, the unimplemented 3D styles) are rendered as
                // a solid fill of the edge's region.
                BorderStyle::Solid
                | BorderStyle::Groove
                | BorderStyle::Ridge
                | BorderStyle::Inset
                | BorderStyle::Outset => {
                    borders[count] = (color, Some(self.frame.border_edge_shape(edge)));
                    count += 1;
                }
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

    /// Draw a single `dashed` or `dotted` border edge.
    ///
    /// The dashes/dots are generated as filled shapes running along the centre of
    /// the edge and are drawn clipped to the edge's region (the same trapezoid
    /// used by the solid path). Clipping means the corners are mitred correctly,
    /// adjacent edges of different styles/colors don't overlap, and any
    /// border-radius is respected.
    fn draw_dashed_border_edge(
        &self,
        scene: &mut impl PaintScene,
        edge: Edge,
        color: Color,
        dotted: bool,
    ) {
        let bb = self.frame.border_box;
        let bw = self.frame.border_width;

        // `thickness` is the width of this border side, `length` is the distance
        // the dashes run along the edge (measured along the outer border box).
        let (thickness, length): (f64, f64) = match edge {
            Edge::Top => (bw.y0, bb.width()),
            Edge::Bottom => (bw.y1, bb.width()),
            Edge::Left => (bw.x0, bb.height()),
            Edge::Right => (bw.x1, bb.height()),
        };

        if thickness <= 0.0 || length <= 0.0 {
            return;
        }

        // Build a rectangle for a dash spanning `[start, end]` along the run,
        // covering the full thickness of the border on the cross axis.
        let dash_rect = |start: f64, end: f64| -> Rect {
            match edge {
                Edge::Top => Rect::new(bb.x0 + start, bb.y0, bb.x0 + end, bb.y0 + thickness),
                Edge::Bottom => Rect::new(bb.x0 + start, bb.y1 - thickness, bb.x0 + end, bb.y1),
                Edge::Left => Rect::new(bb.x0, bb.y0 + start, bb.x0 + thickness, bb.y0 + end),
                Edge::Right => Rect::new(bb.x1 - thickness, bb.y0 + start, bb.x1, bb.y0 + end),
            }
        };

        // Centre point of a dot placed `along` the run (on the centre line of the
        // border thickness).
        let dot_center = |along: f64| -> Point {
            let half = thickness / 2.0;
            match edge {
                Edge::Top => Point::new(bb.x0 + along, bb.y0 + half),
                Edge::Bottom => Point::new(bb.x0 + along, bb.y1 - half),
                Edge::Left => Point::new(bb.x0 + half, bb.y0 + along),
                Edge::Right => Point::new(bb.x1 - half, bb.y0 + along),
            }
        };

        let mut path = BezPath::new();

        if dotted {
            // Dots are circles whose diameter equals the border thickness. They
            // are distributed evenly, each centred within an equally sized cell so
            // that the gaps at either end of the edge are symmetrical.
            let radius = thickness / 2.0;
            let cell_count = (length / (2.0 * thickness)).round().max(1.0);
            let cell = length / cell_count;
            let mut k = 0.0;
            while k < cell_count {
                let center = dot_center((k + 0.5) * cell);
                path.extend(Circle::new(center, radius).path_elements(0.1));
                k += 1.0;
            }
        } else {
            // Dashes are rectangles. They are distributed so that the edge both
            // starts and ends with a dash (covering the corners), with the dashes
            // and gaps all the same length.
            let nominal = 3.0 * thickness;
            let dash_count = ((length / nominal + 1.0) / 2.0).round().max(1.0);
            let segment = length / (2.0 * dash_count - 1.0);
            let mut k = 0.0;
            while k < dash_count {
                let start = 2.0 * k * segment;
                path.extend(dash_rect(start, start + segment).path_elements(0.1));
                k += 1.0;
            }
        }

        let clip = self.frame.border_edge_shape(edge);
        scene.push_clip_layer(self.transform, &clip);
        scene.fill(Fill::NonZero, self.transform, color, None, &path);
        scene.pop_layer();
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
