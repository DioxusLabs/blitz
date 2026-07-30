use crate::node::{Node, NodeData};
use color::{AlphaColor, Srgb};
use keyboard_types::Modifiers;
use std::borrow::Cow;
use style::color::AbsoluteColor;

#[cfg(target_os = "macos")]
pub(crate) const ACTION_MOD: Modifiers = Modifiers::SUPER;
#[cfg(not(target_os = "macos"))]
pub(crate) const ACTION_MOD: Modifiers = Modifiers::CONTROL;

pub type Color = AlphaColor<Srgb>;

/// Decode raw font bytes, decompressing WOFF/WOFF2 if the `woff` feature is enabled.
/// Returns the original slice unchanged for TTF/OTF input, and also on decompression
/// failure. With the `woff` feature disabled, all input passes through unchanged.
pub fn decode_font_bytes(bytes: &[u8]) -> Cow<'_, [u8]> {
    if bytes.len() < 4 {
        return Cow::Borrowed(bytes);
    }
    match &bytes[0..4] {
        #[cfg(feature = "woff")]
        b"wOFF" => wuff::decompress_woff1(bytes)
            .map(Cow::Owned)
            .unwrap_or_else(|_| {
                #[cfg(feature = "tracing")]
                tracing::warn!("Failed to decompress woff1 font");
                Cow::Borrowed(bytes)
            }),
        #[cfg(feature = "woff")]
        b"wOF2" => wuff::decompress_woff2(bytes)
            .map(Cow::Owned)
            .unwrap_or_else(|_| {
                #[cfg(feature = "tracing")]
                tracing::warn!("Failed to decompress woff2 font");
                Cow::Borrowed(bytes)
            }),
        _ => Cow::Borrowed(bytes),
    }
}

#[cfg(feature = "svg")]
use std::sync::{Arc, LazyLock};
#[cfg(feature = "svg")]
use usvg::fontdb;
#[cfg(feature = "svg")]
pub(crate) static FONT_DB: LazyLock<Arc<fontdb::Database>> = LazyLock::new(|| {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    Arc::new(db)
});

/// Which kind of CSS image layer list (`background-image` or `mask-image`) to
/// flush from style to dedicated storage on the node.
#[derive(Clone, Copy, Debug)]
pub enum ImageLayerKind {
    Background,
    Mask,
}

impl ImageLayerKind {
    pub fn image_type(self, idx: usize) -> ImageType {
        match self {
            Self::Background => ImageType::Background(idx),
            Self::Mask => ImageType::Mask(idx),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ImageType {
    Image,
    Background(usize),
    Mask(usize),
}

/// A point
#[derive(Clone, Debug, Copy, Eq, PartialEq)]
pub struct Point<T> {
    /// The x coordinate
    pub x: T,
    /// The y coordinate
    pub y: T,
}

impl Point<f64> {
    pub const ZERO: Self = Point { x: 0.0, y: 0.0 };
}

// Debug print an RcDom
pub fn walk_tree(indent: usize, node: &Node) {
    // Skip all-whitespace text nodes entirely
    if let NodeData::Text(data) = &node.data {
        if data.content.chars().all(|c| c.is_ascii_whitespace()) {
            return;
        }
    }

    print!("{}", " ".repeat(indent));
    let id = node.id;
    match &node.data {
        NodeData::Document(_) => println!("#Document {id}"),

        NodeData::Text(data) => {
            if data.content.chars().all(|c| c.is_ascii_whitespace()) {
                println!("{id} #text: <whitespace>");
            } else {
                let content = data.content.trim();
                if content.len() > 10 {
                    println!(
                        "#text {id}: {}...",
                        content
                            .split_at(content.char_indices().take(10).last().unwrap().0)
                            .0
                            .escape_default()
                    )
                } else {
                    println!("#text {id}: {}", data.content.trim().escape_default())
                }
            }
        }

        NodeData::Comment { .. } => println!("<!-- COMMENT {id} -->"),

        NodeData::AnonymousBlock(_) => println!("{id} AnonymousBlock"),

        NodeData::Element(data) => {
            print!("<{} {id}", data.name.local);
            for attr in data.attrs.iter() {
                print!(" {}=\"{}\"", attr.name.local, attr.value);
            }
            if !node.children.is_empty() {
                println!(">");
            } else {
                println!("/>");
            }
        } // NodeData::Doctype {
          //     ref name,
          //     ref public_id,
          //     ref system_id,
          // } => println!("<!DOCTYPE {} \"{}\" \"{}\">", name, public_id, system_id),
          // NodeData::ProcessingInstruction { .. } => unreachable!(),
    }

    if !node.children.is_empty() {
        for child_id in node.children.iter() {
            walk_tree(indent + 2, node.with(*child_id));
        }

        if let NodeData::Element(data) = &node.data {
            println!("{}</{}>", " ".repeat(indent), data.name.local);
        }
    }
}

/// Parse an SVG image and record its CSS intrinsic dimensions.
///
/// usvg always resolves the root `<svg>` to a concrete size, using the
/// `viewBox` when `width`/`height` are missing or expressed as percentages, and
/// its [`usvg::Tree`] does not retain the raw `width`/`height` attributes. For
/// CSS sizing, however, an SVG in that situation has only an intrinsic aspect
/// ratio and *no* intrinsic width/height, so we inspect the root `<svg>`
/// attributes ourselves.
///
/// To avoid parsing the XML twice, we parse it once into a
/// [`usvg::roxmltree::Document`] (mirroring [`usvg::Tree::from_data`], including
/// SVGZ decompression) and reuse that document both to read the intrinsic
/// dimensions and to build the tree via [`usvg::Tree::from_xmltree`].
#[cfg(feature = "svg")]
pub(crate) fn parse_svg_image(source: &[u8]) -> Result<crate::node::SvgImageData, usvg::Error> {
    use usvg::roxmltree;

    let options = usvg::Options {
        fontdb: Arc::clone(&*FONT_DB),
        ..Default::default()
    };

    // Transparently decompress gzip-compressed SVGZ, as `Tree::from_data` does.
    let decompressed;
    let data = if source.starts_with(&[0x1f, 0x8b]) {
        decompressed = usvg::decompress_svgz(source)?;
        &decompressed[..]
    } else {
        source
    };
    let text = std::str::from_utf8(data).map_err(|_| usvg::Error::NotAnUtf8Str)?;

    let xml_opt = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    };
    let doc = roxmltree::Document::parse_with_options(text, xml_opt)
        .map_err(usvg::Error::ParsingFailed)?;

    let (has_width, has_height) = svg_has_absolute_dimensions(&doc);

    let tree = usvg::Tree::from_xmltree(&doc, &options)?;
    let size = tree.size();
    Ok(crate::node::SvgImageData {
        intrinsic_width: has_width.then(|| size.width()),
        intrinsic_height: has_height.then(|| size.height()),
        tree: Arc::new(tree),
    })
}

/// Returns whether the root `<svg>` element declares absolute (non-percentage)
/// `width` and `height` attributes. A missing attribute defaults to `100%`, so
/// it is treated as non-absolute (i.e. no intrinsic dimension).
#[cfg(feature = "svg")]
fn svg_has_absolute_dimensions(doc: &usvg::roxmltree::Document) -> (bool, bool) {
    let root = doc.root_element();
    (
        root.attribute("width").is_some_and(is_absolute_length),
        root.attribute("height").is_some_and(is_absolute_length),
    )
}

/// Returns whether `value` is a valid absolute (non-percentage) SVG length such
/// as `48`, `12px`, or `2.5em`. Percentages are relative, and unparseable
/// values are not lengths at all, so neither counts as an intrinsic dimension.
#[cfg(feature = "svg")]
fn is_absolute_length(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || value.ends_with('%') {
        return false;
    }
    // Strip a recognised absolute unit (if present) and require the remaining
    // numeric part to be a finite number, mirroring how usvg parses lengths.
    let number = ["px", "pt", "pc", "mm", "cm", "in", "em", "ex"]
        .iter()
        .find_map(|unit| value.strip_suffix(unit))
        .unwrap_or(value)
        .trim();
    number.parse::<f64>().is_ok_and(f64::is_finite)
}

pub trait ToColorColor {
    /// Converts a color into the `AlphaColor<Srgb>` type from the `color` crate
    fn as_color_color(&self) -> Color;
}
impl ToColorColor for AbsoluteColor {
    fn as_color_color(&self) -> Color {
        Color::new(
            *self
                .to_color_space(style::color::ColorSpace::Srgb)
                .raw_components(),
        )
    }
}

#[cfg(all(test, feature = "svg"))]
mod svg_tests {
    use super::parse_svg_image;

    #[test]
    fn viewbox_only_has_no_intrinsic_dimensions() {
        let src = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 485 58"></svg>"#;
        let svg = parse_svg_image(src).unwrap();
        assert_eq!(svg.intrinsic_width, None);
        assert_eq!(svg.intrinsic_height, None);
        // The aspect ratio is still available from the viewBox.
        assert!((svg.aspect_ratio() - (485.0 / 58.0)).abs() < 1e-3);
    }

    #[test]
    fn absolute_dimensions_are_intrinsic() {
        let src = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="16" viewBox="0 0 48 32"></svg>"#;
        let svg = parse_svg_image(src).unwrap();
        assert_eq!(svg.intrinsic_width, Some(24.0));
        assert_eq!(svg.intrinsic_height, Some(16.0));
    }

    #[test]
    fn percentage_dimensions_are_not_intrinsic() {
        let src = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100%" height="50%" viewBox="0 0 200 100"></svg>"#;
        let svg = parse_svg_image(src).unwrap();
        assert_eq!(svg.intrinsic_width, None);
        assert_eq!(svg.intrinsic_height, None);
    }

    #[test]
    fn unit_lengths_are_intrinsic() {
        let src = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24px" height="1.5em" viewBox="0 0 48 32"></svg>"#;
        let svg = parse_svg_image(src).unwrap();
        assert!(svg.intrinsic_width.is_some());
        assert!(svg.intrinsic_height.is_some());
    }

    #[test]
    fn non_numeric_dimensions_are_not_intrinsic() {
        let src = br#"<svg xmlns="http://www.w3.org/2000/svg" width="auto" height="foo" viewBox="0 0 200 100"></svg>"#;
        let svg = parse_svg_image(src).unwrap();
        assert_eq!(svg.intrinsic_width, None);
        assert_eq!(svg.intrinsic_height, None);
    }

    #[test]
    fn is_absolute_length_validates_numbers() {
        use super::is_absolute_length;
        assert!(is_absolute_length("48"));
        assert!(is_absolute_length(" 12px "));
        assert!(is_absolute_length("2.5em"));
        assert!(is_absolute_length("1e3"));

        assert!(!is_absolute_length("100%"));
        assert!(!is_absolute_length("auto"));
        assert!(!is_absolute_length("foo"));
        assert!(!is_absolute_length(""));
        assert!(!is_absolute_length("px"));
    }
}

/// Creates an markup5ever::QualName.
/// Given a local name and an optional namespace
#[macro_export]
macro_rules! qual_name {
    ($local:tt $(, $ns:ident)?) => {
        $crate::QualName {
            prefix: None,
            ns: $crate::ns!($($ns)?),
            local: $crate::local_name!($local),
        }
    };
}
