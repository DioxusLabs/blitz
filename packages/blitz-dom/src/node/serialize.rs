//! Serialization of nodes to HTML, and of inline `<svg>` subtrees to SVG source for usvg.

use html_escape::{encode_quoted_attribute_to_string, encode_text_to_string};
use markup5ever::{Namespace, QualName, local_name, ns};
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
    /// Replace `currentColor` in attribute values with the element's computed `color`
    resolve_current_color: bool,
    /// Write the `xmlns` declarations that XML needs and HTML serialization leaves out, so that
    /// the element and attribute namespaces in the output match the DOM
    xml_namespaces: bool,
    /// Leave out the root element's `transform` attribute, which is applied as a CSS transform
    skip_root_transform: bool,
    /// Leave the root background to CSS box painting rather than usvg
    skip_root_background: bool,
}

impl SerializeOpts {
    const fn html(style: OutputStyle) -> Self {
        Self {
            style,
            resolve_current_color: false,
            xml_namespaces: false,
            skip_root_transform: false,
            skip_root_background: false,
        }
    }
}

const XLINK_NS: &str = "http://www.w3.org/1999/xlink";

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

/// Writes an attribute's name for XML output. An attribute in a namespace other than the
/// predeclared `xml` and `xlink` ones gets a prefix declared on its element, recorded in
/// `declared` so that each prefix is declared once per element. Returns `false` for attributes
/// that can't be written as XML: namespace declarations, which are written from the element and
/// attribute namespaces instead, and names that aren't valid XML names.
fn write_xml_attr_name(
    name: &QualName,
    declared: &mut Vec<(String, Namespace)>,
    writer: &mut String,
) -> bool {
    match name.ns {
        ns!(xmlns) => return false,
        ns!() if name.local.contains(':') || name.local.as_str() == "xmlns" => return false,
        ns!() => {}
        ns!(xml) => writer.push_str("xml:"),
        ns!(xlink) => writer.push_str("xlink:"),
        _ => {
            let prefix = match declared.iter().find(|(_, ns)| *ns == name.ns) {
                Some((prefix, _)) => prefix.clone(),
                None => {
                    let mut prefix = name
                        .prefix
                        .as_deref()
                        .filter(|prefix| !matches!(*prefix, "xml" | "xmlns" | "xlink"))
                        .unwrap_or("ns")
                        .to_string();
                    let base_len = prefix.len();
                    let mut suffix = 0;
                    while declared.iter().any(|(declared, _)| *declared == prefix) {
                        suffix += 1;
                        prefix.truncate(base_len);
                        prefix.push_str(&suffix.to_string());
                    }
                    writer.push_str("xmlns:");
                    writer.push_str(&prefix);
                    writer.push_str("=\"");
                    encode_quoted_attribute_to_string(&name.ns, writer);
                    writer.push_str("\" ");
                    declared.push((prefix.clone(), name.ns.clone()));
                    prefix
                }
            };
            writer.push_str(&prefix);
            writer.push(':');
        }
    }
    writer.push_str(&name.local);
    true
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

    /// Serializes this `<svg>` subtree as a standalone SVG document for usvg, with
    /// `currentColor` resolved against each element's computed `color`.
    #[cfg(feature = "svg")]
    pub(crate) fn svg_source(&self) -> String {
        let mut output = String::new();
        let opts = SerializeOpts {
            style: OutputStyle::Normal,
            resolve_current_color: true,
            xml_namespaces: true,
            skip_root_transform: true,
            skip_root_background: true,
        };
        self.write_outer_html_in_style(&mut output, opts, 0);
        output
    }

    fn write_outer_html_in_style(&self, writer: &mut String, opts: SerializeOpts, nesting: usize) {
        const INDENT: &str = "  ";
        let style = opts.style;
        let has_children = !self.children.is_empty();
        let current_color = opts
            .resolve_current_color
            .then(|| self.primary_styles())
            .flatten()
            .map(|style| style.clone_color())
            .map(|color| color.to_css_string());

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
                if matches!(style, OutputStyle::Pretty) {
                    for _ in 0..nesting {
                        writer.push_str(INDENT);
                    }
                }
                writer.push('<');
                writer.push_str(&data.name.local);

                if opts.xml_namespaces {
                    let parent_ns = (nesting > 0)
                        .then(|| self.parent.and_then(|id| self.tree()[id].element_data()))
                        .flatten()
                        .map(|parent| &parent.name.ns);
                    if data.name.ns != *parent_ns.unwrap_or(&ns!()) {
                        writer.push_str(" xmlns=\"");
                        encode_quoted_attribute_to_string(&data.name.ns, writer);
                        writer.push('"');
                    }
                    if nesting == 0 {
                        writer.push_str(" xmlns:xlink=\"");
                        writer.push_str(XLINK_NS);
                        writer.push('"');
                    }
                }

                let mut declared_prefixes = Vec::new();
                for attr in data.attrs() {
                    if opts.skip_root_transform
                        && nesting == 0
                        && attr.name.ns == ns!()
                        && attr.name.local == local_name!("transform")
                    {
                        continue;
                    }
                    writer.push(' ');
                    if opts.xml_namespaces {
                        if !write_xml_attr_name(&attr.name, &mut declared_prefixes, writer) {
                            writer.pop();
                            continue;
                        }
                    } else {
                        write_attr_name(&attr.name, writer);
                    }
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
                    if opts.skip_root_background
                        && nesting == 0
                        && attr.name.ns == ns!()
                        && attr.name.local == local_name!("style")
                    {
                        if !attr.value.trim().is_empty() {
                            writer.push(';');
                        }
                        writer.push_str("background-color:initial!important");
                    }
                    writer.push('"');
                }
                if opts.skip_root_background
                    && nesting == 0
                    && data.attr(local_name!("style")).is_none()
                {
                    writer.push_str(" style=\"background-color:initial!important\"");
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

#[cfg(all(test, feature = "svg"))]
mod tests {
    use markup5ever::{LocalName, Namespace, Prefix, QualName, local_name, ns};

    use crate::{Attribute, BaseDocument, DocumentConfig, ElementData, NodeData};

    fn qual_name(prefix: Option<&str>, ns: Namespace, local: &str) -> QualName {
        QualName::new(prefix.map(Prefix::from), ns, LocalName::from(local))
    }

    #[test]
    fn svg_source_declares_namespaces() {
        let mut doc = BaseDocument::new(DocumentConfig::default());
        let mut element = |name: QualName, attrs: Vec<Attribute>| {
            doc.create_node(NodeData::Element(Box::new(ElementData::new(name, attrs))))
        };
        let svg = element(qual_name(None, ns!(svg), "svg"), vec![]);
        let foreign = element(
            qual_name(None, Namespace::from("urn:notsvg"), "rect"),
            vec![],
        );
        let rect = element(
            QualName::new(None, ns!(svg), local_name!("rect")),
            vec![
                Attribute {
                    name: qual_name(Some("svg"), ns!(svg), "href"),
                    value: "#a".into(),
                },
                Attribute {
                    name: qual_name(Some("xlink"), Namespace::from("urn:notxlink"), "href"),
                    value: "#b".into(),
                },
            ],
        );
        doc.mutate().append_children(svg, &[foreign, rect]);

        let source = doc.get_node(svg).unwrap().svg_source();
        assert_eq!(
            source,
            concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" style="background-color:initial!important">"#,
                r#"<rect xmlns="urn:notsvg" />"#,
                r##"<rect xmlns:svg="http://www.w3.org/2000/svg" svg:href="#a" xmlns:ns="urn:notxlink" ns:href="#b" />"##,
                "</svg>",
            )
        );
        assert!(crate::util::parse_svg(source.as_bytes(), None).is_ok());
    }

    #[test]
    fn svg_source_leaves_root_background_to_css() {
        for background in [None, Some(""), Some("background-color:red")] {
            let mut doc = BaseDocument::new(DocumentConfig::default());
            let mut attrs = vec![Attribute {
                name: qual_name(None, ns!(), "background-color"),
                value: "blue".into(),
            }];
            if let Some(value) = background {
                attrs.push(Attribute {
                    name: qual_name(None, ns!(), "style"),
                    value: value.into(),
                });
            }
            let svg = doc.create_node(NodeData::Element(Box::new(ElementData::new(
                qual_name(None, ns!(svg), "svg"),
                attrs,
            ))));
            let style = doc.create_node(NodeData::Element(Box::new(ElementData::new(
                qual_name(None, ns!(svg), "style"),
                vec![],
            ))));
            let text = doc.create_text_node("svg { background-color: green }");
            doc.mutate().append_children(style, &[text]);
            doc.mutate().append_children(svg, &[style]);

            let source = doc.get_node(svg).unwrap().svg_source();
            let image = crate::util::parse_svg(source.as_bytes(), None).unwrap();
            assert!(image.tree.root().children().is_empty(), "{source}");
        }
    }
}
