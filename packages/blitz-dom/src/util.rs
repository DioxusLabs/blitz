use crate::node::{Node, NodeData};
use color::{AlphaColor, Srgb};
use keyboard_types::Modifiers;
use std::borrow::Cow;
use style::color::AbsoluteColor;

#[cfg(feature = "svg")]
use parley::FontContext;
#[cfg(feature = "svg")]
use std::sync::{Arc, Mutex};
#[cfg(feature = "svg")]
use usvg::fontdb;

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

/// SVG text font resolution.
///
/// usvg keeps its own `fontdb`, filled by default only from system fonts.
/// Resolving through Fontique instead means `DocumentConfig::font_ctx` covers
/// SVG text as well as HTML text.
#[cfg(feature = "svg")]
mod svg_fonts {
    use parley::FontContext;
    use parley::fontique::{
        Attributes, Blob, FontStyle, FontWeight, FontWidth, GenericFamily, QueryFamily, QueryFont,
        QueryStatus,
    };
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use usvg::fontdb;

    /// Faces already copied into usvg's database, keyed by blob address and index.
    type Cache = Arc<Mutex<HashMap<(usize, u32), fontdb::ID>>>;

    pub(crate) fn resolver(font_ctx: Arc<Mutex<FontContext>>) -> usvg::FontResolver<'static> {
        let cache = Cache::default();
        usvg::FontResolver {
            select_font: select_font(font_ctx.clone(), cache.clone()),
            select_fallback: select_fallback(font_ctx, cache),
        }
    }

    fn select_font(
        font_ctx: Arc<Mutex<FontContext>>,
        cache: Cache,
    ) -> usvg::FontSelectionFn<'static> {
        Box::new(move |font, db| {
            let families: Vec<QueryFamily<'_>> = font.families().iter().map(family).collect();
            let attributes = Attributes::new(
                width(font.stretch()),
                style(font.style()),
                FontWeight::new(font.weight() as f32),
            );

            let mut ctx = font_ctx.lock().unwrap();
            let ctx = &mut *ctx;
            let mut query = ctx.collection.query(&mut ctx.source_cache);
            query.set_families(families);
            query.set_attributes(attributes);

            let mut found = None;
            query.matches_with(|candidate| {
                found = Some(candidate.clone());
                QueryStatus::Stop
            });
            drop(query);

            adopt(&cache, db, &found?)
        })
    }

    fn select_fallback(
        font_ctx: Arc<Mutex<FontContext>>,
        cache: Cache,
    ) -> usvg::FallbackSelectionFn<'static> {
        Box::new(move |ch, used, db| {
            let mut ctx = font_ctx.lock().unwrap();
            let ctx = &mut *ctx;
            let mut query = ctx.collection.query(&mut ctx.source_cache);
            query.set_families([
                QueryFamily::from(GenericFamily::SansSerif),
                QueryFamily::from(GenericFamily::Serif),
                QueryFamily::from(GenericFamily::Monospace),
            ]);

            let mut found = None;
            query.matches_with(|candidate| {
                if covers(candidate, ch) {
                    found = Some(candidate.clone());
                    QueryStatus::Stop
                } else {
                    QueryStatus::Continue
                }
            });
            drop(query);

            let id = adopt(&cache, db, &found?)?;
            (!used.contains(&id)).then_some(id)
        })
    }

    fn covers(font: &QueryFont, ch: char) -> bool {
        font.charmap()
            .is_some_and(|charmap| charmap.map(ch).is_some())
    }

    /// Copy a Fontique face into usvg's database, once, and return its id.
    fn adopt(
        cache: &Cache,
        db: &mut Arc<fontdb::Database>,
        font: &QueryFont,
    ) -> Option<fontdb::ID> {
        let key = (font.blob.as_ref().as_ptr() as usize, font.index);
        if let Some(id) = cache.lock().unwrap().get(&key) {
            return Some(*id);
        }

        let source = fontdb::Source::Binary(Arc::new(Bytes(font.blob.clone())));
        let ids = Arc::make_mut(db).load_font_source(source);
        let id = *ids.get(font.index as usize).or_else(|| ids.first())?;
        cache.lock().unwrap().insert(key, id);
        Some(id)
    }

    fn family(family: &usvg::FontFamily) -> QueryFamily<'_> {
        match family {
            usvg::FontFamily::Named(name) => QueryFamily::Named(name),
            usvg::FontFamily::Serif => GenericFamily::Serif.into(),
            usvg::FontFamily::SansSerif => GenericFamily::SansSerif.into(),
            usvg::FontFamily::Cursive => GenericFamily::Cursive.into(),
            usvg::FontFamily::Fantasy => GenericFamily::Fantasy.into(),
            usvg::FontFamily::Monospace => GenericFamily::Monospace.into(),
        }
    }

    fn style(style: usvg::FontStyle) -> FontStyle {
        match style {
            usvg::FontStyle::Normal => FontStyle::Normal,
            usvg::FontStyle::Italic => FontStyle::Italic,
            usvg::FontStyle::Oblique => FontStyle::Oblique(None),
        }
    }

    fn width(stretch: usvg::FontStretch) -> FontWidth {
        match stretch {
            usvg::FontStretch::UltraCondensed => FontWidth::ULTRA_CONDENSED,
            usvg::FontStretch::ExtraCondensed => FontWidth::EXTRA_CONDENSED,
            usvg::FontStretch::Condensed => FontWidth::CONDENSED,
            usvg::FontStretch::SemiCondensed => FontWidth::SEMI_CONDENSED,
            usvg::FontStretch::Normal => FontWidth::NORMAL,
            usvg::FontStretch::SemiExpanded => FontWidth::SEMI_EXPANDED,
            usvg::FontStretch::Expanded => FontWidth::EXPANDED,
            usvg::FontStretch::ExtraExpanded => FontWidth::EXTRA_EXPANDED,
            usvg::FontStretch::UltraExpanded => FontWidth::ULTRA_EXPANDED,
        }
    }

    /// `fontdb::Source::Binary` wants `AsRef<[u8]>`; `Blob` derefs to a slice.
    struct Bytes(Blob<u8>);

    impl AsRef<[u8]> for Bytes {
        fn as_ref(&self) -> &[u8] {
            self.0.as_ref()
        }
    }
}

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

/// Parse an SVG image.
#[cfg(feature = "svg")]
pub(crate) fn parse_svg_image(
    source: &[u8],
    font_ctx: Arc<Mutex<FontContext>>,
) -> Result<crate::node::SvgImageData, usvg::Error> {
    let options = usvg::Options {
        // Left empty: the resolver adds faces from the document's collection.
        fontdb: Arc::new(fontdb::Database::new()),
        font_resolver: svg_fonts::resolver(font_ctx),
        ..Default::default()
    };
    crate::node::SvgImageData::from_data(source, &options)
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
    fn missing_height_is_computed_from_width_and_viewbox_ratio() {
        let src = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" width="200"><rect width="100%" height="100%" fill="green"/></svg>"#;
        let svg = parse_svg_image(src).unwrap();
        assert_eq!(svg.intrinsic_width(), Some(200.0));
        assert_eq!(svg.intrinsic_height(), None);
        assert_eq!(svg.viewbox_aspect_ratio(), Some(1.0));
        assert_eq!(svg.tree.size().width(), 200.0);
        assert_eq!(svg.intrinsic_size(), (200.0, 200.0));
    }

    #[test]
    fn viewbox_only_has_no_intrinsic_dimensions() {
        let src = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 485 58"></svg>"#;
        let svg = parse_svg_image(src).unwrap();
        assert_eq!(svg.intrinsic_width(), None);
        assert_eq!(svg.intrinsic_height(), None);
        // The aspect ratio is still available from the viewBox.
        assert!((svg.aspect_ratio() - (485.0 / 58.0)).abs() < 1e-3);
    }

    #[test]
    fn absolute_dimensions_are_intrinsic() {
        let src = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="16" viewBox="0 0 48 32"></svg>"#;
        let svg = parse_svg_image(src).unwrap();
        assert_eq!(svg.intrinsic_width(), Some(24.0));
        assert_eq!(svg.intrinsic_height(), Some(16.0));
    }

    #[test]
    fn percentage_dimensions_are_not_intrinsic() {
        let src = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100%" height="50%" viewBox="0 0 200 100"></svg>"#;
        let svg = parse_svg_image(src).unwrap();
        assert_eq!(svg.intrinsic_width(), None);
        assert_eq!(svg.intrinsic_height(), None);
    }

    #[test]
    fn unit_lengths_are_intrinsic() {
        let src = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24px" height="1.5em" viewBox="0 0 48 32"></svg>"#;
        let svg = parse_svg_image(src).unwrap();
        assert!(svg.intrinsic_width().is_some());
        assert!(svg.intrinsic_height().is_some());
    }

    #[test]
    fn non_numeric_dimensions_are_not_intrinsic() {
        let src = br#"<svg xmlns="http://www.w3.org/2000/svg" width="auto" height="foo" viewBox="0 0 200 100"></svg>"#;
        let svg = parse_svg_image(src).unwrap();
        assert_eq!(svg.intrinsic_width(), None);
        assert_eq!(svg.intrinsic_height(), None);
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
