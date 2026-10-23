//! Serialization of nodes to HTML, and of inline `<svg>` subtrees to SVG source for usvg.

use html_escape::{encode_quoted_attribute_to_string, encode_text_to_string};
use markup5ever::{QualName, ns};
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
    /// Declare the `xlink` prefix on the root element so that the output is well-formed XML
    declare_xlink_ns: bool,
    /// Serialize as XML (for usvg) rather than with the HTML fragment serialization
    /// algorithm: empty elements are self-closing and there are no void elements.
    xml: bool,
}

impl SerializeOpts {
    const fn html(style: OutputStyle) -> Self {
        Self {
            style,
            resolve_current_color: false,
            declare_xlink_ns: false,
            xml: false,
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
    /// <html><head></head><body><main id="main"><div class="arbitrary-class"></div></main></body></html>
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
    ///   <head>
    ///   </head>
    ///   <body>
    ///     <main id="main">
    ///       <div class="arbitrary-class">
    ///       </div>
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
            declare_xlink_ns: true,
            xml: true,
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
                } else if opts.xml {
                    encode_text_to_string(data.content.as_str(), writer);
                } else {
                    // U+00A0 is `&nbsp;` in HTML; inside foreign content (SVG,
                    // which is also serialized to be parsed as XML) the numeric
                    // reference, as XML has no `nbsp` entity.
                    let in_html = self
                        .parent
                        .and_then(|id| self.tree()[id].data.downcast_element())
                        .is_none_or(|el| el.name.ns == ns!(html));
                    escape_text(data.content.as_str(), in_html, writer);
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

                if opts.declare_xlink_ns
                    && nesting == 0
                    && !data.attrs().iter().any(|attr| is_xlink_ns_decl(&attr.name))
                {
                    writer.push_str(" xmlns:xlink=\"");
                    writer.push_str(XLINK_NS);
                    writer.push('"');
                }

                for attr in data.attrs() {
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
                // In XML an empty element is self-closing. In HTML a void element
                // has no end tag (and no children), and every other element has
                // one, even when it is empty: `<div />` is not self-closing in
                // HTML and would swallow the following siblings when parsed back.
                let self_closing = opts.xml && !has_children;
                if self_closing {
                    writer.push_str(" /");
                }
                writer.push('>');
                if matches!(style, OutputStyle::Pretty) {
                    writer.push('\n');
                }

                let is_void =
                    !opts.xml && data.name.ns == ns!(html) && is_void_element(&data.name.local);
                if !self_closing && !is_void {
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

/// The void elements of the HTML fragment serialization algorithm, which have no end tag.
fn is_void_element(local: &str) -> bool {
    matches!(
        local,
        "area"
            | "base"
            | "basefont"
            | "bgsound"
            | "br"
            | "col"
            | "embed"
            | "frame"
            | "hr"
            | "img"
            | "input"
            | "keygen"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

/// Escape text for HTML fragment serialization: `&`, `<`, `>` and U+00A0.
fn escape_text(text: &str, in_html: bool, writer: &mut String) {
    for c in text.chars() {
        match c {
            '&' => writer.push_str("&amp;"),
            '<' => writer.push_str("&lt;"),
            '>' => writer.push_str("&gt;"),
            '\u{a0}' if in_html => writer.push_str("&nbsp;"),
            '\u{a0}' => writer.push_str("&#160;"),
            c => writer.push(c),
        }
    }
}
