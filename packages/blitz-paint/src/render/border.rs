use anyrender::PaintScene;
use blitz_dom::node::SpecialElementData;
use kurbo::{BezPath, Cap, Circle, Insets, Join, PathEl, Point, Rect, Shape as _, Stroke, Vec2};
use peniko::{Color, Fill};
use smallvec::SmallVec;
use style::{
    computed_values::border_collapse::T as BorderCollapse,
    values::computed::{BorderStyle, OutlineStyle},
};

use crate::{color::ToColorColor as _, kurbo_css::Edge, render::ElementCx};

/// The WCAG contrast ratio (>= 1) between two colours, matching Chrome's
/// `color_utils::GetContrastRatio`.
fn contrast_ratio(a: Color, b: Color) -> f32 {
    // `relative_luminance` is defined on `OpaqueColor`; discard the alpha (which
    // it ignores anyway) with `split`.
    let la = a.split().0.relative_luminance() + 0.05;
    let lb = b.split().0.relative_luminance() + 0.05;
    if la > lb { la / lb } else { lb / la }
}

/// A darker version of a colour (mirrors WebKit/Blink's `Color::Dark`): the
/// brightest channel is reduced by a fixed amount and the others scaled to match,
/// preserving hue.
fn darken(color: Color) -> Color {
    let [r, g, b, a] = color.components;
    let v = r.max(g).max(b);
    if v == 0.0 {
        return color;
    }
    let multiplier = ((v - 0.33) / v).max(0.0);
    Color::new([r * multiplier, g * multiplier, b * multiplier, a])
}

/// A lighter version of a colour (mirrors WebKit/Blink's `Color::Light`): the
/// brightest channel is increased by a fixed amount and the others scaled to
/// match, preserving hue.
fn lighten(color: Color) -> Color {
    let [r, g, b, a] = color.components;
    let v = r.max(g).max(b);
    if v == 0.0 {
        // Pure black: WebKit uses a fixed dark grey (0x545454).
        return Color::new([0.33, 0.33, 0.33, a]);
    }
    let multiplier = (v + 0.33).min(1.0) / v;
    Color::new([r * multiplier, g * multiplier, b * multiplier, a])
}

/// The colour of a single edge of an `inset`/`outset` border.
///
/// An `outset` border is raised: the top/left edges use the "lighter" shade and
/// the bottom/right edges the "darker" shade (`inset` is the reverse). The exact
/// shading matches Chrome's `CalculateInsetOutsetColor`:
///
/// * The darker edge is always darkened.
/// * The lighter edge keeps the base colour unchanged, *except* for colours dark
///   enough that the darkened edge wouldn't have enough contrast against the base
///   colour, which are lightened instead so the bevel stays visible.
fn beveled_edge_color(color: Color, edge: Edge, inset: bool) -> Color {
    let top_or_left = matches!(edge, Edge::Top | Edge::Left);
    let should_darken = top_or_left == inset;

    let dark_color = darken(color);
    if should_darken {
        return dark_color;
    }

    // Lighter edge. Chrome uses the base colour as-is unless the colour is dark
    // enough that the darkened edge lacks contrast against it. The red/green
    // shortcut mirrors Chrome: for those colours the contrast is always
    // sufficient, so the base colour is used without a contrast check.
    let [r, g, _, _] = color.components;
    if r >= 150.0 / 255.0 || g >= 92.0 / 255.0 {
        return color;
    }

    // Lighten only when the darkened edge would be too low-contrast to read.
    const MIN_CONTRAST_RATIO: f32 = 1.75;
    if contrast_ratio(color, dark_color) < MIN_CONTRAST_RATIO {
        lighten(color)
    } else {
        color
    }
}

/// The (outer half, inner half) colours of a single edge of a `groove`/`ridge`
/// border.
///
/// A `groove` looks carved into the page: its outer half is shaded like `inset`
/// and its inner half like `outset`. `ridge` is the reverse (raised).
fn grooved_edge_colors(color: Color, edge: Edge, ridge: bool) -> (Color, Color) {
    let outer = beveled_edge_color(color, edge, !ridge);
    let inner = beveled_edge_color(color, edge, ridge);
    (outer, inner)
}

/// The `(dash, gap)` lengths of a dashed border, as multiples of the border
/// thickness.
///
/// Matches Chrome: borders that are at least 3px use a 2:1 dash-to-gap ratio,
/// while thinner borders use longer dashes and gaps (3:2) so they still read as
/// dashes rather than dots or a solid line. `thickness` and `scale` are in device
/// pixels; the 3px threshold is applied in CSS pixels.
fn dashed_ratios(thickness: f64, scale: f64) -> (f64, f64) {
    if thickness >= 3.0 * scale {
        (2.0, 1.0)
    } else {
        (3.0, 2.0)
    }
}

/// Return `count` points spaced `spacing` apart (by arc length) along `path`,
/// starting at its beginning. Used to place dots along a rounded border.
fn sample_points_along_path(path: &BezPath, spacing: f64, count: usize) -> Vec<Point> {
    // Flatten to a polyline so we can walk it by arc length.
    let mut poly: Vec<Point> = Vec::new();
    kurbo::flatten(path.iter(), 0.1, |el| match el {
        PathEl::MoveTo(p) | PathEl::LineTo(p) => poly.push(p),
        PathEl::ClosePath => {
            if let Some(&first) = poly.first() {
                poly.push(first);
            }
        }
        _ => {}
    });

    let mut points = Vec::with_capacity(count);
    if poly.len() < 2 {
        return points;
    }

    let mut seg = 0;
    let mut seg_start = 0.0;
    let mut seg_len = (poly[1] - poly[0]).hypot();
    for i in 0..count {
        let target = i as f64 * spacing;
        while seg + 2 < poly.len() && target > seg_start + seg_len {
            seg_start += seg_len;
            seg += 1;
            seg_len = (poly[seg + 1] - poly[seg]).hypot();
        }
        let t = if seg_len > 0.0 {
            ((target - seg_start) / seg_len).clamp(0.0, 1.0)
        } else {
            0.0
        };
        points.push(poly[seg].lerp(poly[seg + 1], t));
    }
    points
}

impl ElementCx<'_, '_> {
    /// Draw all borders for a node
    pub(crate) fn draw_border(&self, scene: &mut impl PaintScene) {
        let style = &*self.style;
        let border = style.get_border();
        let current_color = style.clone_color();

        // Fast path: a uniform-width, single-color solid border whose corners are
        // all circular arcs (equal x and y radius — sharp rectangles, ordinary
        // rounded rectangles and circles all qualify) can be drawn as a single
        // stroke along its center-line instead of a fill per edge. Besides being
        // faster, this works around Vello seam artifacts at the quarter-arc joins
        // when a thin circular/elliptical border is drawn as the
        // border-box/padding-box annulus.
        //
        // A full ellipse (`border-radius: 50%` on a non-square box) has the same
        // seam problem but no rounded-rect representation, so it is stroked as an
        // ellipse.
        let all_solid = border.border_top_style == BorderStyle::Solid
            && border.border_right_style == BorderStyle::Solid
            && border.border_bottom_style == BorderStyle::Solid
            && border.border_left_style == BorderStyle::Solid;
        if all_solid {
            let color = border
                .border_top_color
                .resolve_to_absolute(&current_color)
                .as_srgb_color();
            let same_color = [
                &border.border_right_color,
                &border.border_bottom_color,
                &border.border_left_color,
            ]
            .iter()
            .all(|c| c.resolve_to_absolute(&current_color).as_srgb_color() == color);
            let bw = self.frame.border_width;
            let uniform_width = bw.x0 == bw.y0 && bw.x0 == bw.x1 && bw.x0 == bw.y1 && bw.x0 > 0.0;
            if same_color && uniform_width && color.components[3] > 0.0 {
                let bb = self.frame.border_box;
                let pb = self.frame.padding_box;
                let width = (bb.width() - pb.width()) / 2.0;
                // Miter join keeps sharp corners square. It has no effect on rounded
                // corners, whose arcs meet the straight edges tangentially.
                let stroke = Stroke::new(width).with_join(Join::Miter);

                if self.frame.is_uniform_corner_border() {
                    let radii = self.frame.border_radii;
                    // The stroke matches the CSS border shape only when each corner is
                    // sharp or its radius is at least the border width (outer radius
                    // `r`, inner radius `r - width`, concentric). For 0 < r < width CSS
                    // draws a rounded outer edge against a sharp inner corner, which a
                    // stroke can't represent — leave those to the per-edge fill.
                    let is_exact = |r: Vec2| r.x == 0.0 || r.x >= width;
                    let stroke_is_exact = is_exact(radii.top_left)
                        && is_exact(radii.top_right)
                        && is_exact(radii.bottom_right)
                        && is_exact(radii.bottom_left);
                    if stroke_is_exact {
                        let half_width = width / 2.0;
                        // Center-line rect, and corner radii shrunk to match.
                        let centerline_box = bb - Insets::uniform(half_width);
                        let centerline_radius = |r: Vec2| (r.x - half_width).max(0.0);
                        let radii = kurbo::RoundedRectRadii::new(
                            centerline_radius(radii.top_left),
                            centerline_radius(radii.top_right),
                            centerline_radius(radii.bottom_right),
                            centerline_radius(radii.bottom_left),
                        );
                        let rr = kurbo::RoundedRect::from_rect(centerline_box, radii);
                        scene.stroke(&stroke, self.transform, color, None, &rr);
                        return;
                    }
                } else if self.frame.is_elliptical_border() {
                    // Center-line radii, straight from the boxes.
                    let rx = (bb.width() + pb.width()) / 4.0;
                    let ry = (bb.height() + pb.height()) / 4.0;
                    let e = kurbo::Ellipse::new(bb.center(), (rx, ry), 0.0);
                    scene.stroke(&stroke, self.transform, color, None, &e);
                    return;
                }
            }
        }

        // (colour, path) pairs to be filled. Several entries may share a colour;
        // they are grouped before filling so that adjacent same-coloured regions
        // are drawn together, avoiding anti-aliasing seams between them.
        //
        // At most 8 entries: 4 edges, each of which can contribute 2 (the outer
        // and inner halves of a `groove`/`ridge`).
        let mut borders: SmallVec<[(Color, BezPath); 8]> = SmallVec::new();

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
                BorderStyle::Dotted => self.draw_dotted_border_edge(scene, edge, color),
                BorderStyle::Dashed => self.draw_dashed_border_edge(scene, edge, color),

                // A double border is two solid lines separated by a gap, splitting
                // the border width into three equal parts (outer line / gap / inner
                // line). Both lines share a color, so both rings are placed into a
                // single path.
                BorderStyle::Double => {
                    // Needs at least 3px (one device pixel per line and per gap) to
                    // render as two lines; thinner borders fall back to a solid
                    // fill, matching browser behaviour.
                    let path = if self.edge_width(edge) < 3.0 * self.scale {
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
                    borders.push((color, path));
                }

                // `inset`/`outset` are solid, but each edge is shaded lighter or
                // darker to give a bevelled (3D) appearance.
                BorderStyle::Inset | BorderStyle::Outset => {
                    let inset = edge_style == BorderStyle::Inset;
                    let shade = beveled_edge_color(color, edge, inset);
                    borders.push((shade, self.frame.border_edge_shape(edge)));
                }

                // `groove`/`ridge` split each edge into an outer and an inner half,
                // each shaded as if it were `inset`/`outset`, producing a carved or
                // raised ridge.
                BorderStyle::Groove | BorderStyle::Ridge => {
                    let ridge = edge_style == BorderStyle::Ridge;
                    let (outer, inner) = grooved_edge_colors(color, edge, ridge);
                    borders.push((
                        outer,
                        self.frame.border_slice(0.0, 0.5).border_edge_shape(edge),
                    ));
                    borders.push((
                        inner,
                        self.frame.border_slice(0.5, 1.0).border_edge_shape(edge),
                    ));
                }

                // Solid fills the whole edge region with the border colour.
                BorderStyle::Solid => {
                    borders.push((color, self.frame.border_edge_shape(edge)));
                }
            }
        }

        if borders.is_empty() {
            return;
        }

        // Group together identical colors by sorting, then fill each group as a
        // single path.
        borders.sort_unstable_by(|a, b| {
            a.0.components
                .partial_cmp(&b.0.components)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut start = 0;
        while start < borders.len() {
            let color = borders[start].0;
            let mut path = std::mem::take(&mut borders[start].1);
            let mut next = start + 1;
            while next < borders.len() && borders[next].0 == color {
                path.extend(&borders[next].1);
                next += 1;
            }
            scene.fill(Fill::NonZero, self.transform, color, None, &path);
            start = next;
        }
    }

    /// The border width (in device pixels) of a single edge.
    fn edge_width(&self, edge: Edge) -> f64 {
        match edge {
            Edge::Top => self.frame.border_width.y0,
            Edge::Bottom => self.frame.border_width.y1,
            Edge::Left => self.frame.border_width.x0,
            Edge::Right => self.frame.border_width.x1,
        }
    }

    /// Draw a single `dashed` border edge.
    ///
    /// The dashes are produced by stroking the centre line of the border with a
    /// dash pattern (butt caps give them flat, square ends). Everything is clipped
    /// to the edge's region (the same trapezoid used by the solid path) so corners
    /// are mitred, adjacent edges of different colors don't overlap, and any
    /// border-radius is respected.
    fn draw_dashed_border_edge(&self, scene: &mut impl PaintScene, edge: Edge, color: Color) {
        let thickness = self.edge_width(edge);
        if thickness <= 0.0 {
            return;
        }

        let (dash_ratio, gap_ratio) = dashed_ratios(thickness, self.scale);

        // Work out the centre line to stroke and the dash/gap lengths along it.
        let (centerline, dash, gap) = if self.frame.has_border_radius() {
            // Rounded corners: stroke the rounded centre line running through the
            // whole perimeter. Every edge uses the same centre line and pattern, so
            // dashes stay continuous and aligned as they wrap around each corner.
            // Dash and gap keep their ratio, sized so a whole number of periods fit
            // exactly around the perimeter (kurbo merges the dash across the seam).
            let mut centerline = self.frame.border_slice(0.0, 0.5).padding_box_path();
            centerline.close_path();
            let perimeter = centerline.perimeter(0.1);
            if perimeter <= 0.0 {
                return;
            }
            let period0 = (dash_ratio + gap_ratio) * thickness;
            let count = (perimeter / period0).round().max(1.0);
            let period = perimeter / count;
            let dash = period * dash_ratio / (dash_ratio + gap_ratio);
            (centerline, dash, period - dash)
        } else {
            // Square corners: stroke a straight line through the middle of the edge,
            // corner to corner. Dash and gap keep their ratio but are scaled so the
            // edge both starts and ends with a dash (covering the corners).
            let bb = self.frame.border_box;
            let half = thickness / 2.0;
            let (start, end, length) = match edge {
                Edge::Top => (
                    Point::new(bb.x0, bb.y0 + half),
                    Point::new(bb.x1, bb.y0 + half),
                    bb.width(),
                ),
                Edge::Bottom => (
                    Point::new(bb.x0, bb.y1 - half),
                    Point::new(bb.x1, bb.y1 - half),
                    bb.width(),
                ),
                Edge::Left => (
                    Point::new(bb.x0 + half, bb.y0),
                    Point::new(bb.x0 + half, bb.y1),
                    bb.height(),
                ),
                Edge::Right => (
                    Point::new(bb.x1 - half, bb.y0),
                    Point::new(bb.x1 - half, bb.y1),
                    bb.height(),
                ),
            };
            if length <= 0.0 {
                return;
            }
            let dash0 = dash_ratio * thickness;
            let gap0 = gap_ratio * thickness;
            // `count` dashes with `count - 1` gaps between them.
            let count = ((length + gap0) / (dash0 + gap0)).round().max(1.0);
            let r = dash_ratio / gap_ratio;
            let gap = length / (count * r + count - 1.0);

            let mut line = BezPath::new();
            line.move_to(start);
            line.line_to(end);
            (line, r * gap, gap)
        };

        let stroke = Stroke::new(thickness)
            .with_caps(Cap::Butt)
            .with_dashes(0.0, [dash, gap]);
        let clip = self.frame.border_edge_shape(edge);
        scene.push_clip_layer(self.transform, &clip);
        scene.stroke(&stroke, self.transform, color, None, &centerline);
        scene.pop_layer();
    }

    /// Draw a single `dotted` border edge.
    ///
    /// Dots are filled circles (diameter == border thickness). Unlike dashes they
    /// can't be produced by stroking, because kurbo doesn't emit zero-length dashes
    /// (so a round-capped dash pattern would render nothing); drawing them
    /// explicitly also lets us anchor a dot in each square corner. Everything is
    /// clipped to the edge's region, as for the other styles.
    fn draw_dotted_border_edge(&self, scene: &mut impl PaintScene, edge: Edge, color: Color) {
        let thickness = self.edge_width(edge);
        if thickness <= 0.0 {
            return;
        }
        let radius = thickness / 2.0;

        let mut path = BezPath::new();
        if self.frame.has_border_radius() {
            // Rounded corners: dots spaced evenly around the rounded centre line so
            // the ring wraps seamlessly around the corners.
            let mut centerline = self.frame.border_slice(0.0, 0.5).padding_box_path();
            centerline.close_path();
            let perimeter = centerline.perimeter(0.1);
            if perimeter <= 0.0 {
                return;
            }
            let count = (perimeter / (2.0 * thickness)).round().max(1.0);
            let spacing = perimeter / count;
            for center in sample_points_along_path(&centerline, spacing, count as usize) {
                path.extend(Circle::new(center, radius).path_elements(0.1));
            }
        } else {
            // Square corners: a dot is anchored in each corner (both ends of the
            // edge, inset by the radius so it sits snugly in the corner) and the
            // rest are spread evenly between them.
            let bb = self.frame.border_box;
            let length = match edge {
                Edge::Top | Edge::Bottom => bb.width(),
                Edge::Left | Edge::Right => bb.height(),
            };
            if length <= 0.0 {
                return;
            }
            let dot_center = |along: f64| -> Point {
                match edge {
                    Edge::Top => Point::new(bb.x0 + along, bb.y0 + radius),
                    Edge::Bottom => Point::new(bb.x0 + along, bb.y1 - radius),
                    Edge::Left => Point::new(bb.x0 + radius, bb.y0 + along),
                    Edge::Right => Point::new(bb.x1 - radius, bb.y0 + along),
                }
            };

            let span = length - thickness;
            if span <= 0.0 {
                // Edge too short to fit two dots; place a single centred dot.
                let center = dot_center(length / 2.0);
                path.extend(Circle::new(center, radius).path_elements(0.1));
            } else {
                // Aim for a centre-to-centre spacing of about two dot diameters,
                // then adjust it so the first and last dots land in the corners.
                let gaps = (span / (2.0 * thickness)).round().max(1.0) as usize;
                let spacing = span / gaps as f64;
                for i in 0..=gaps {
                    let center = dot_center(radius + i as f64 * spacing);
                    path.extend(Circle::new(center, radius).path_elements(0.1));
                }
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
