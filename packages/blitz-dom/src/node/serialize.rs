//! Serialization of nodes to HTML, and of inline `<svg>` subtrees to SVG source for usvg.

use html_escape::{encode_quoted_attribute_to_string, encode_text_to_string};
use markup5ever::{QualName, local_name, ns};
use style_traits::values::ToCss;
use stylo_taffy::CopyStyleGetters;

use super::{Node, NodeData};

#[derive(Clone, Copy)]
enum OutputStyle {
    Normal,
    Pretty,
}

#[derive(Clone, Copy)]
struct SerializeOpts {
    style: OutputStyle,
    /// Resolve the CSS cascade for SVG-internal elements: `currentColor` in attribute values,
    /// and `fill`/`stroke`/`opacity`/`display` emitted as literal presentation attributes
    /// (see `write_outer_html_in_style`'s `NodeData::Element` arm).
    resolve_svg_cascade: bool,
    /// Declare the `xlink` prefix on the root element so that the output is well-formed XML
    declare_xlink_ns: bool,
}

impl SerializeOpts {
    const fn html(style: OutputStyle) -> Self {
        Self {
            style,
            resolve_svg_cascade: false,
            declare_xlink_ns: false,
        }
    }
}

const XLINK_NS: &str = "http://www.w3.org/1999/xlink";

/// Serializes a computed `fill`/`stroke` value as the corresponding SVG presentation attribute
/// value, resolving any `currentColor` it carries against the element's own computed `color`.
#[cfg(feature = "svg")]
fn svg_paint_attr_value(
    paint: &style::values::computed::SVGPaint,
    current_color: &style::color::AbsoluteColor,
) -> String {
    use style::values::computed::SVGPaintKind;
    match &paint.kind {
        SVGPaintKind::Color(color) => color
            .resolve_to_absolute(current_color)
            .into_srgb_legacy()
            .to_css_string(),
        // `none`, `url(..)`, `context-fill`/`context-stroke`: none of these involve an
        // ambiguous colour-space serialization, so the derived `ToCss` impl is fine as-is.
        _ => paint.to_css_string(),
    }
}

/// Writes an attribute's serialized name, per the HTML fragment serialization algorithm
fn write_attr_name(name: &QualName, writer: &mut String) {
    match name.ns {
        ns!() | ns!(html) => {}
        ns!(xml) => writer.push_str("xml:"),
        ns!(xmlns) if name.local.as_str() == "xmlns" => {}
        ns!(xmlns) => writer.push_str("xmlns:"),
        ns!(xlink) => writer.push_str("xlink:"),
        _ => {
            if let Some(prefix) = &name.prefix {
                writer.push_str(prefix);
                writer.push(':');
            }
        }
    }
    writer.push_str(&name.local);
}

fn is_xlink_ns_decl(name: &QualName) -> bool {
    (name.ns == ns!(xmlns) && name.local.as_str() == "xlink")
        || name.local.as_str() == "xmlns:xlink"
}

impl Node {
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
        self.write_outer_html_in_style(writer, SerializeOpts::html(OutputStyle::Normal), 0);
    }

    pub fn write_outer_html_pretty(&self, writer: &mut String) {
        self.write_outer_html_in_style(writer, SerializeOpts::html(OutputStyle::Pretty), 0);
    }

    /// Serializes this `<svg>` subtree as a standalone SVG document for usvg, with the CSS
    /// cascade applied to every element: `currentColor` resolved against each element's
    /// computed `color`, and `fill`/`stroke`/`opacity`/`display` emitted as literal
    /// presentation attributes from each element's own computed style (`usvg` never sees the
    /// host page's `<style>` rules, so this is the only way a class-based rule, an inherited
    /// property, or an `@keyframes`-driven value reaches an inline `<svg>`'s internal elements).
    #[cfg(feature = "svg")]
    pub(crate) fn svg_source(&self) -> String {
        let mut output = String::new();
        let opts = SerializeOpts {
            style: OutputStyle::Normal,
            resolve_svg_cascade: true,
            declare_xlink_ns: true,
        };
        self.write_outer_html_in_style(&mut output, opts, 0);
        output
    }

    fn write_outer_html_in_style(&self, writer: &mut String, opts: SerializeOpts, nesting: usize) {
        const INDENT: &str = "  ";
        let style = opts.style;
        let has_children = !self.children.is_empty();
        let primary_styles = opts
            .resolve_svg_cascade
            .then(|| self.primary_styles())
            .flatten();
        let current_color_abs = primary_styles.as_deref().map(|s| s.clone_color());
        let current_color = current_color_abs
            // usvg's attribute color parser only understands the legacy CSS Color syntax
            // (`#rrggbb`, `rgb()`/`rgba()`, named colors); a computed `color` that uses modern
            // CSS Color 4 function syntax (`color(srgb ..)`, `oklch(..)`, etc. -- which Stylo
            // always produces for e.g. `color-mix()`, see `color::mix::mix_many`'s
            // `RESULT_IN_MODERN_SYNTAX` flag) would otherwise be silently dropped by usvg,
            // falling back to each SVG property's own initial value instead of the resolved
            // color. Force the legacy syntax here so the substitution always round-trips
            // through usvg regardless of how the color was computed.
            .map(|color| color.into_srgb_legacy().to_css_string());

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
                let in_raw_text_element = self
                    .parent
                    .and_then(|id| self.tree()[id].data.downcast_element())
                    .is_some_and(|el| {
                        // Documents are parsed with scripting disabled, so `<noscript>` is not raw text
                        el.name.ns == ns!(html)
                            && matches!(
                                el.name.local.as_ref(),
                                "style"
                                    | "script"
                                    | "xmp"
                                    | "iframe"
                                    | "noembed"
                                    | "noframes"
                                    | "plaintext"
                            )
                    });
                if in_raw_text_element {
                    writer.push_str(data.content.as_str());
                } else {
                    encode_text_to_string(data.content.as_str(), writer);
                }
                if matches!(style, OutputStyle::Pretty) {
                    writer.push('\n');
                }
            }
            NodeData::Element(data) => {
                // The whole CSS cascade is otherwise invisible to SVG-internal elements --
                // only literal DOM attributes (and `currentColor`, above) are serialized. A
                // computed `display:none` removes the element (and its subtree) entirely,
                // matching every other rendering path; `fill`/`stroke`/`opacity` are threaded
                // through below as literal presentation attributes, so a class-based or
                // `@keyframes`-driven value reaches `usvg` the same way a literal attribute
                // always did. (`fill`/`stroke` presentation *attributes* are synthesized back
                // into the cascade as low-priority hints --
                // `synthesize_presentational_hints_for_legacy_attributes` in `stylo.rs` -- so
                // the computed value already reflects the right cascade result whether it came
                // from CSS or from a literal attribute. `transform` is not threaded through:
                // unlike these, it requires composing with any existing literal SVG `transform`
                // attribute and resolving a percentage `transform-origin`, left for later work.)
                let svg_cascade = opts.resolve_svg_cascade && data.name.ns == ns!(svg);
                if svg_cascade
                    && primary_styles
                        .as_deref()
                        .is_some_and(|s| s.clone_display().is_none())
                {
                    return;
                }

                if matches!(style, OutputStyle::Pretty) {
                    for _ in 0..nesting {
                        writer.push_str(INDENT);
                    }
                }
                writer.push('<');
                writer.push_str(&data.name.local);

                if opts.declare_xlink_ns
                    && nesting == 0
                    && !data.attrs().iter().any(|attr| is_xlink_ns_decl(&attr.name))
                {
                    writer.push_str(" xmlns:xlink=\"");
                    writer.push_str(XLINK_NS);
                    writer.push('"');
                }

                for attr in data.attrs() {
                    if svg_cascade
                        && (attr.name.local == local_name!("fill")
                            || attr.name.local == local_name!("stroke")
                            || attr.name.local == local_name!("opacity"))
                    {
                        // Replaced below with the resolved computed value.
                        continue;
                    }
                    writer.push(' ');
                    write_attr_name(&attr.name, writer);
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

                if let (true, Some(cascade_style), Some(current_color_abs)) = (
                    svg_cascade,
                    primary_styles.as_deref(),
                    current_color_abs.as_ref(),
                ) {
                    let svg = cascade_style.get_inherited_svg();
                    writer.push_str(" fill=\"");
                    encode_quoted_attribute_to_string(
                        svg_paint_attr_value(svg.get_fill(), current_color_abs),
                        writer,
                    );
                    writer.push('"');
                    writer.push_str(" stroke=\"");
                    encode_quoted_attribute_to_string(
                        svg_paint_attr_value(svg.get_stroke(), current_color_abs),
                        writer,
                    );
                    writer.push('"');
                    let opacity = cascade_style.clone_opacity();
                    if opacity != 1.0 {
                        writer.push_str(" opacity=\"");
                        encode_quoted_attribute_to_string(opacity.to_string(), writer);
                        writer.push('"');
                    }
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
                        self.tree()[child_id].write_outer_html_in_style(writer, opts, nesting + 1);
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
}

#[cfg(test)]
mod tests {
    use crate::{Attribute, BaseDocument, DocumentConfig, qual_name};
    use blitz_traits::shell::{ColorScheme, Viewport};
    use style::stylesheets::Origin;

    /// `svg_source()`'s `currentColor` substitution must round-trip through usvg's attribute
    /// color parser, which only understands the legacy CSS Color syntax (`#rrggbb`,
    /// `rgb()`/`rgba()`, named colors). A computed `color` that resolves through a
    /// `color-mix()` always serializes in the modern `color(srgb ..)` function syntax (Stylo
    /// sets `RESULT_IN_MODERN_SYNTAX` for every `color-mix()`, regardless of its inputs) --
    /// without forcing the legacy syntax here, this value would be silently dropped by usvg.
    #[test]
    fn svg_source_resolves_current_color_through_color_mix() {
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(400, 200, 1.0, ColorScheme::Light)),
            ..Default::default()
        });
        let root_id = doc.root_node().id;
        let mut mutator = doc.mutate();
        let html = mutator.create_element(qual_name!("html"), vec![]);
        let body = mutator.create_element(qual_name!("body"), vec![]);
        let svg = mutator.create_element(
            qual_name!("svg", svg),
            vec![Attribute {
                name: qual_name!("style"),
                value: "color: color-mix(in srgb, #5b8def 70%, #fff)".to_string(),
            }],
        );
        let circle = mutator.create_element(
            qual_name!("circle", svg),
            vec![Attribute {
                name: qual_name!("fill"),
                value: "currentColor".to_string(),
            }],
        );
        mutator.append_children(svg, &[circle]);
        mutator.append_children(body, &[svg]);
        mutator.append_children(html, &[body]);
        mutator.append_children(root_id, &[html]);
        drop(mutator);
        doc.resolve(0.0);

        let source = doc.nodes[svg].svg_source();
        // color-mix(in srgb, #5b8def 70%, #fff) = rgb(140, 175, 244); must appear in legacy
        // rgb() syntax, never as `color(srgb ..)`.
        assert!(
            source.contains("fill=\"rgb(140, 175, 244)\""),
            "expected legacy rgb() currentColor substitution, got: {source}"
        );
        assert!(
            !source.contains("color("),
            "modern color() syntax leaked into svg_source(), usvg cannot parse it: {source}"
        );
    }

    /// The core bug this fixes: a CSS class-selector rule for `fill`/`stroke` was invisible to
    /// `svg_source()` -- it only ever saw literal DOM attributes, so a class rule never reached
    /// an inline `<svg>`'s internal elements at all; the element always fell back to whatever a
    /// literal attribute said (or SVG's own initial value, `fill:black`, with no attribute at
    /// all) regardless of what CSS said.
    #[test]
    fn svg_source_class_rule_overrides_literal_fill_attribute() {
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(200, 100, 1.0, ColorScheme::Light)),
            ..Default::default()
        });
        let root_id = doc.root_node().id;
        let mut mutator = doc.mutate();
        let html = mutator.create_element(qual_name!("html"), vec![]);
        let body = mutator.create_element(qual_name!("body"), vec![]);
        let svg = mutator.create_element(qual_name!("svg", svg), vec![]);
        let rect = mutator.create_element(
            qual_name!("rect", svg),
            vec![
                Attribute {
                    name: qual_name!("class"),
                    value: "corpo".to_string(),
                },
                // A literal attribute the class rule must still win over.
                Attribute {
                    name: qual_name!("fill"),
                    value: "black".to_string(),
                },
            ],
        );
        mutator.append_children(svg, &[rect]);
        mutator.append_children(body, &[svg]);
        mutator.append_children(html, &[body]);
        mutator.append_children(root_id, &[html]);
        drop(mutator);
        // `add_stylesheet_for_node` stores the sheet as an element's `special_data`, mimicking
        // a real `<style>` tag, so it needs an element node (any element works for cascade
        // purposes; a real `<style>` element isn't required here).
        let sheet = doc.make_stylesheet(".corpo { fill: #d97757; }", Origin::Author);
        doc.add_stylesheet_for_node(sheet, html);
        doc.resolve(0.0);

        let source = doc.nodes[svg].svg_source();
        assert!(
            source.contains("fill=\"rgb(217, 119, 87)\""),
            "class rule must reach the SVG-internal <rect>, got: {source}"
        );
    }

    /// A literal `fill` attribute with no competing CSS rule must still resolve back to its own
    /// value -- the presentational-hint synthesis in `stylo.rs` is what makes this possible once
    /// the computed value (rather than the literal attribute) is always what gets serialized.
    #[test]
    fn svg_source_preserves_literal_fill_without_css_override() {
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(200, 100, 1.0, ColorScheme::Light)),
            ..Default::default()
        });
        let root_id = doc.root_node().id;
        let mut mutator = doc.mutate();
        let html = mutator.create_element(qual_name!("html"), vec![]);
        let body = mutator.create_element(qual_name!("body"), vec![]);
        let svg = mutator.create_element(qual_name!("svg", svg), vec![]);
        let rect = mutator.create_element(
            qual_name!("rect", svg),
            vec![Attribute {
                name: qual_name!("fill"),
                value: "#336699".to_string(),
            }],
        );
        mutator.append_children(svg, &[rect]);
        mutator.append_children(body, &[svg]);
        mutator.append_children(html, &[body]);
        mutator.append_children(root_id, &[html]);
        drop(mutator);
        doc.resolve(0.0);

        let source = doc.nodes[svg].svg_source();
        assert!(
            source.contains("fill=\"rgb(51, 102, 153)\""),
            "literal fill attribute must survive when no CSS rule overrides it, got: {source}"
        );
    }

    /// `opacity`/`display:none` driven by CSS (e.g. `@keyframes`) must also reach SVG-internal
    /// elements: a non-1 computed `opacity` is emitted as a literal presentation attribute, and
    /// a computed `display:none` removes the element (and its subtree) from the serialized
    /// document entirely, the same as it would anywhere else.
    #[test]
    fn svg_source_applies_computed_opacity_and_removes_display_none_subtree() {
        let mut doc = BaseDocument::new(DocumentConfig {
            viewport: Some(Viewport::new(200, 100, 1.0, ColorScheme::Light)),
            ..Default::default()
        });
        let root_id = doc.root_node().id;
        let mut mutator = doc.mutate();
        let html = mutator.create_element(qual_name!("html"), vec![]);
        let body = mutator.create_element(qual_name!("body"), vec![]);
        let svg = mutator.create_element(qual_name!("svg", svg), vec![]);
        let faded = mutator.create_element(
            qual_name!("rect", svg),
            vec![Attribute {
                name: qual_name!("class"),
                value: "faded".to_string(),
            }],
        );
        let hidden = mutator.create_element(
            qual_name!("g", svg),
            vec![Attribute {
                name: qual_name!("class"),
                value: "hidden".to_string(),
            }],
        );
        let hidden_child = mutator.create_element(qual_name!("rect", svg), vec![]);
        mutator.append_children(hidden, &[hidden_child]);
        mutator.append_children(svg, &[faded, hidden]);
        mutator.append_children(body, &[svg]);
        mutator.append_children(html, &[body]);
        mutator.append_children(root_id, &[html]);
        drop(mutator);
        let sheet = doc.make_stylesheet(
            ".faded { opacity: 0.4; } .hidden { display: none; }",
            Origin::Author,
        );
        doc.add_stylesheet_for_node(sheet, html);
        doc.resolve(0.0);

        let source = doc.nodes[svg].svg_source();
        assert!(
            source.contains("opacity=\"0.4\""),
            "computed opacity must be emitted as a literal attribute, got: {source}"
        );
        assert!(
            !source.contains("<g"),
            "a display:none element must be removed from the serialized document, got: {source}"
        );
    }
}
