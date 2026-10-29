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
    /// Replace `currentColor` in attribute values with the element's computed `color`
    resolve_current_color: bool,
    /// Declare the `xlink` prefix on the root element so that the output is well-formed XML
    declare_xlink_ns: bool,
    /// Write an expanded SVG `<use>` (see `crate::svg_use`) as a `<g>` around its clone
    #[cfg(feature = "svg")]
    expand_svg_use: bool,
}

impl SerializeOpts {
    const fn html(style: OutputStyle) -> Self {
        Self {
            style,
            resolve_current_color: false,
            declare_xlink_ns: false,
            #[cfg(feature = "svg")]
            expand_svg_use: false,
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
    /// `currentColor` resolved against each element's computed `color`. Elements it references
    /// by `url(#id)` or `href="#id"` that are defined elsewhere in the document (a gradient or
    /// filter in an icon sprite) are copied into a trailing `<defs>`.
    #[cfg(feature = "svg")]
    pub(crate) fn svg_source(&self) -> String {
        let mut output = String::new();
        let opts = SerializeOpts {
            style: OutputStyle::Normal,
            resolve_current_color: true,
            declare_xlink_ns: true,
            expand_svg_use: true,
        };
        self.write_outer_html_in_style(&mut output, opts, 0);

        let mut defined = std::collections::HashSet::new();
        let mut referenced = Vec::new();
        self.collect_svg_ids(&mut defined, &mut referenced);
        let mut defs = String::new();
        while let Some(id) = referenced.pop() {
            if !defined.insert(id.clone()) {
                continue;
            }
            if let Some(node) = self.find_element_by_id(&id) {
                node.write_outer_html_in_style(&mut defs, opts, 1);
                node.collect_svg_ids(&mut defined, &mut referenced);
            }
        }
        if !defs.is_empty() {
            let close = output.rfind("</").unwrap_or(output.len());
            output.insert_str(close, &format!("<defs>{defs}</defs>"));
        }
        output
    }

    /// The ids this subtree defines, and those it references by `url(#id)` or by `href="#id"`
    /// (except on an expanded `<use>`, whose clone is written in its place).
    #[cfg(feature = "svg")]
    fn collect_svg_ids(
        &self,
        defined: &mut std::collections::HashSet<String>,
        referenced: &mut Vec<String>,
    ) {
        if self.is_dropped_svg_use() {
            return;
        }
        if let Some(data) = self.element_data() {
            if let Some(id) = data.id.as_deref() {
                defined.insert(id.to_string());
            }
            let expanded_use = self.is_expanded_svg_use();
            for attr in data.attrs() {
                let mut rest = attr.value.as_str();
                while let Some(start) = rest.find("url(") {
                    rest = &rest[start + 4..];
                    let inner = rest.split(')').next().unwrap_or("");
                    let inner = inner.trim().trim_matches(|c| c == '"' || c == '\'');
                    if let Some(id) = inner.strip_prefix('#') {
                        referenced.push(id.to_string());
                    }
                }
                if attr.name.local == local_name!("href") && !expanded_use {
                    if let Some(id) = attr.value.strip_prefix('#') {
                        referenced.push(id.to_string());
                    }
                }
            }
        }
        for &child_id in &self.children {
            self.tree()[child_id].collect_svg_ids(defined, referenced);
        }
    }

    /// The first element in tree order with this `id`.
    #[cfg(feature = "svg")]
    fn find_element_by_id(&self, id: &str) -> Option<&Node> {
        let mut root = self;
        while let Some(parent) = root.parent {
            root = &self.tree()[parent];
        }
        let mut stack = vec![root.id];
        while let Some(node_id) = stack.pop() {
            let node = &self.tree()[node_id];
            if node
                .element_data()
                .is_some_and(|el| el.id.as_deref() == Some(id))
            {
                return Some(node);
            }
            stack.extend(node.children.iter().rev().copied());
        }
        None
    }

    /// Whether this is an SVG `<use>` element expanded by `crate::svg_use`.
    #[cfg(feature = "svg")]
    fn is_expanded_svg_use(&self) -> bool {
        self.element_data()
            .is_some_and(|el| el.name.ns == ns!(svg) && el.name.local == local_name!("use"))
            && self
                .children
                .iter()
                .any(|&id| self.tree()[id].is_svg_use_clone())
    }

    /// Whether this is an SVG `<use>` inside a clone that was left unexpanded: a circular
    /// reference, which renders nothing.
    #[cfg(feature = "svg")]
    fn is_dropped_svg_use(&self) -> bool {
        let is_use = self
            .element_data()
            .is_some_and(|el| el.name.ns == ns!(svg) && el.name.local == local_name!("use"));
        if !is_use || self.is_expanded_svg_use() {
            return false;
        }
        let mut current = self.parent;
        while let Some(id) = current {
            let node = &self.tree()[id];
            if node.is_svg_use_clone() {
                return true;
            }
            current = node.parent;
        }
        false
    }

    #[cfg(feature = "svg")]
    fn is_svg_use_clone(&self) -> bool {
        self.element_data().is_some_and(|el| {
            el.attrs()
                .iter()
                .any(|attr| *attr.name.local == *crate::svg_use::USE_CLONE_ATTR)
        })
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
                #[cfg(feature = "svg")]
                if opts.expand_svg_use && self.is_dropped_svg_use() {
                    return;
                }
                #[cfg(feature = "svg")]
                let expanded_use = opts.expand_svg_use && self.is_expanded_svg_use();
                #[cfg(not(feature = "svg"))]
                let expanded_use = false;
                // An expanded `<use>` is a group around its clone, translated by its `x`/`y`.
                let tag: &str = if expanded_use { "g" } else { &data.name.local };
                writer.push('<');
                writer.push_str(tag);

                if opts.declare_xlink_ns
                    && nesting == 0
                    && !data.attrs().iter().any(|attr| is_xlink_ns_decl(&attr.name))
                {
                    writer.push_str(" xmlns:xlink=\"");
                    writer.push_str(XLINK_NS);
                    writer.push('"');
                }

                for attr in data.attrs() {
                    #[cfg(feature = "svg")]
                    if opts.expand_svg_use
                        && attr.name.ns == ns!()
                        && *attr.name.local == *crate::svg_use::USE_CLONE_ATTR
                    {
                        continue;
                    }
                    if expanded_use
                        && (attr.name.local == local_name!("href")
                            || (attr.name.ns == ns!()
                                && matches!(
                                    &*attr.name.local,
                                    "x" | "y" | "width" | "height" | "transform"
                                )))
                    {
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
                if expanded_use {
                    let attr = |name: &str| {
                        data.attrs()
                            .iter()
                            .find(|attr| attr.name.ns == ns!() && &*attr.name.local == name)
                            .map(|attr| attr.value.trim())
                    };
                    let translate = format!(
                        "translate({} {})",
                        attr("x").unwrap_or("0"),
                        attr("y").unwrap_or("0")
                    );
                    let transform = match attr("transform") {
                        Some(transform) => format!("{transform} {translate}"),
                        None => translate,
                    };
                    writer.push_str(" transform=\"");
                    encode_quoted_attribute_to_string(&transform, writer);
                    writer.push('"');
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
                    writer.push_str(tag);
                    writer.push('>');
                    if matches!(style, OutputStyle::Pretty) {
                        writer.push('\n');
                    }
                }
            }
        }
    }
}
