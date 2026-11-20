//! Presentational hints: the style that legacy HTML and SVG attributes such as `bgcolor`,
//! `width` or `cellpadding` map to.
//! <https://html.spec.whatwg.org/multipage/rendering.html#presentational-hints>

use markup5ever::{local_name, ns};
use selectors::sink::Push;
use style::Atom;
use style::applicable_declarations::ApplicableDeclarationBlock;
use style::color::AbsoluteColor;
use style::properties::{Importance, PropertyDeclaration, PropertyDeclarationBlock};
use style::rule_tree::{CascadeLevel, CascadeOrigin, StyleSourceBorrow};
use style::servo_arc::Arc as ServoArc;
use style::shared_lock::Locked;
use style::stylesheets::layer_rule::LayerOrder;
use style::values::computed::text::TextAlign as StyloTextAlign;
use style::values::generics::{NonNegative, length::Size};
use style::values::specified::{
    BorderSideWidth, LengthPercentage, Margin, NoCalcLength, NoCalcPercentage, XLang,
};

use crate::node::{ElementData, Node};

/// Whether the local name `$tag` is one of the listed names.
macro_rules! matches_tag {
    ($tag:expr, $($name:tt)|+) => {
        matches!(*$tag, $(local_name!($name))|+)
    };
}

/// One declaration per listed longhand, each set to `value`.
macro_rules! apply_to_all {
    ($value:expr => $($property:ident),+ $(,)?) => {{
        let value = $value;
        [$(PropertyDeclaration::$property(value.clone())),+]
    }};
}

/// A length of `px` CSS pixels.
fn px(px: f32) -> LengthPercentage {
    LengthPercentage::Length(NoCalcLength::from_px(px))
}

/// The value of a `width`/`height` property for a `<length-percentage>`.
fn size(length: LengthPercentage) -> Size<NonNegative<LengthPercentage>> {
    Size::LengthPercentage(NonNegative(length))
}

/// The HTML "rules for parsing non-negative integers", which most pixel-valued attributes use.
fn parse_pixels(value: &str) -> Option<u32> {
    style::servo::attr::parse_unsigned_integer(value.chars()).ok()
}

/// The declaration block that collects an element's hints.
struct Hints(PropertyDeclarationBlock);

impl Hints {
    fn push(&mut self, declaration: PropertyDeclaration) {
        self.0.push(declaration, Importance::Normal);
    }

    fn extend(&mut self, declarations: impl IntoIterator<Item = PropertyDeclaration>) {
        for declaration in declarations {
            self.push(declaration);
        }
    }
}

/// The declaration blocks an element's presentational attributes map to.
#[derive(Default)]
pub(crate) struct PresentationalHints {
    /// The hints for the element itself.
    pub own: Option<ServoArc<Locked<PropertyDeclarationBlock>>>,
    /// For a table, the padding its `cellpadding` attribute gives its cells. Kept on the table
    /// so that the cells' hints don't depend on their ancestors.
    pub cell_padding: Option<ServoArc<Locked<PropertyDeclarationBlock>>>,
}

/// Push the declaration blocks of the node's presentational hints into `hints`.
pub(crate) fn synthesize<'a, V>(node: &'a Node, hints: &mut V)
where
    V: Push<ApplicableDeclarationBlock<'a>>,
{
    let Some(elem) = node.data.downcast_element() else {
        return;
    };
    let element_hints = presentational_hints(node, elem);
    let cell_padding = if matches_tag!(&elem.name.local, "td" | "th") {
        table_of_cell(node)
            .and_then(|table| Some(presentational_hints(table, table.data.downcast_element()?)))
            .and_then(|table| table.cell_padding.as_ref())
    } else {
        None
    };
    for block in [element_hints.own.as_ref(), cell_padding]
        .into_iter()
        .flatten()
    {
        hints.push(ApplicableDeclarationBlock::from_declarations(
            StyleSourceBorrow::from_declarations(block.borrow_arc()),
            CascadeLevel::new(CascadeOrigin::PresHints),
            LayerOrder::root(),
        ));
    }
}

/// The element's presentational hints, built on first use.
fn presentational_hints<'a>(node: &'a Node, elem: &'a ElementData) -> &'a PresentationalHints {
    elem.presentational_hints
        .get_or_init(|| compute_presentational_hints(node, elem))
}

/// The table whose `cellpadding` applies to a cell: its first ancestor that isn't a row or row
/// group, if that is a `table`.
/// https://html.spec.whatwg.org/multipage/rendering.html#tables-2
fn table_of_cell(cell: &Node) -> Option<&Node> {
    let mut ancestor = cell.parent_node();
    while let Some(node) = ancestor {
        let name = &node.data.downcast_element()?.name.local;
        if !matches_tag!(name, "tr" | "tbody" | "thead" | "tfoot") {
            return (*name == local_name!("table")).then_some(node);
        }
        ancestor = node.parent_node();
    }
    None
}

fn compute_presentational_hints(node: &Node, elem: &ElementData) -> PresentationalHints {
    let tag = &elem.name.local;

    let mut hints = Hints(PropertyDeclarationBlock::new());

    fn parse_color_attr(value: &str) -> Option<(u8, u8, u8, f32)> {
        if !value.starts_with('#') {
            return None;
        }

        let value = &value[1..];
        if value.len() == 3 {
            let r = u8::from_str_radix(&value[0..1], 16).ok()?;
            let g = u8::from_str_radix(&value[1..2], 16).ok()?;
            let b = u8::from_str_radix(&value[2..3], 16).ok()?;
            return Some((r, g, b, 1.0));
        }

        if value.len() == 6 {
            let r = u8::from_str_radix(&value[0..2], 16).ok()?;
            let g = u8::from_str_radix(&value[2..4], 16).ok()?;
            let b = u8::from_str_radix(&value[4..6], 16).ok()?;
            return Some((r, g, b, 1.0));
        }

        None
    }

    /// The HTML "rules for parsing dimension values" -- Stylo's
    /// implementation of them -- packaged as a specified
    /// `<length-percentage>`. `ignoring_zero` selects the spec's separate
    /// "maps to the dimension property (ignoring zero)" mapping, where a
    /// zero value is dropped rather than honoured.
    ///
    /// https://html.spec.whatwg.org/multipage/common-microsyntaxes.html#rules-for-parsing-dimension-values
    fn parse_size_attr(value: &str, ignoring_zero: bool) -> Option<LengthPercentage> {
        use style::servo::attr::{LengthOrPercentageOrAuto, parse_length, parse_nonzero_length};
        let parsed = if ignoring_zero {
            parse_nonzero_length(value)
        } else {
            parse_length(value)
        };
        match parsed {
            LengthOrPercentageOrAuto::Length(length) => Some(px(length.to_f32_px())),
            LengthOrPercentageOrAuto::Percentage(fraction) => Some(LengthPercentage::Percentage(
                NoCalcPercentage::new(fraction),
            )),
            LengthOrPercentageOrAuto::Auto => None,
        }
    }

    /// Parse the value of an SVG `width`/`height` presentation attribute.
    /// Unlike the legacy HTML dimension attributes, these accept any CSS
    /// <length-percentage> (e.g. `1em`), and a unitless number means user
    /// units, which map to CSS px.
    fn parse_svg_size_attr(value: &str) -> Option<LengthPercentage> {
        use style_traits::ParsingMode;

        let value = value.trim();
        if let Some(number) = value.strip_suffix('%') {
            let val: f32 = number.trim().parse().ok()?;
            return (val >= 0.0)
                .then(|| LengthPercentage::Percentage(NoCalcPercentage::new(val / 100.0)));
        }

        // Split into number and unit: the unit is the trailing run of
        // ASCII alphabetic characters (this never eats into a scientific
        // exponent such as `1e3`, which ends in a digit).
        let number_len = value
            .trim_end_matches(|c: char| c.is_ascii_alphabetic())
            .len();
        let (number, unit) = value.split_at(number_len);
        let val: f32 = number.trim().parse().ok().filter(|v| *v >= 0.0)?;
        let length = if unit.is_empty() {
            NoCalcLength::from_px(val)
        } else {
            NoCalcLength::parse_dimension_with_flags(ParsingMode::DEFAULT, false, val, unit).ok()?
        };
        Some(LengthPercentage::Length(length))
    }

    /// The rules for parsing a legacy font size: an integer from 1 to 7, either
    /// absolute or relative (`+n`, `-n`) to the default size 3.
    ///
    /// https://html.spec.whatwg.org/multipage/rendering.html#rules-for-parsing-a-legacy-font-size
    fn parse_legacy_font_size(value: &str) -> Option<u8> {
        use style::str::{HTML_SPACE_CHARACTERS, read_numbers};

        let mut chars = value
            .trim_start_matches(HTML_SPACE_CHARACTERS)
            .chars()
            .peekable();
        let sign = chars.next_if(|&c| matches!(c, '+' | '-'));
        let number = match read_numbers(chars) {
            (Some(number), _) => number,
            // No digits
            (None, 0) => return None,
            // Too many digits for an i64, but every number above 7 gives the same size
            (None, _) => i64::MAX,
        };
        let size = match sign {
            Some('+') => number.saturating_add(3),
            Some('-') => 3 - number,
            _ => number,
        };
        Some(size.clamp(1, 7) as u8)
    }

    /// `value` parsed as a declaration of `property`, as an author style would be.
    fn parse_declaration(property: &str, value: &str) -> Option<PropertyDeclaration> {
        use style::properties::{
            PropertyId, SourcePropertyDeclaration, parse_one_declaration_into,
        };
        use style::stylesheets::{CssRuleType, Origin};
        use style_traits::ParsingMode;

        let property = PropertyId::parse_enabled_for_all_content(property).ok()?;
        let mut declarations = SourcePropertyDeclaration::default();
        parse_one_declaration_into(
            &mut declarations,
            property,
            value,
            Origin::Author,
            &crate::url::DocumentUrl::default().url_extra_data(),
            None,
            ParsingMode::DEFAULT,
            style::context::QuirksMode::NoQuirks,
            CssRuleType::Style,
        )
        .ok()?;
        declarations.drain().declarations.next()
    }

    /// The `face` attribute's value parsed as a `font-family` list, as Gecko and
    /// Blink parse it: CSS-wide keywords and `var()` are not family lists.
    fn parse_font_face_attr(value: &str) -> Option<PropertyDeclaration> {
        parse_declaration("font-family", value)
            .filter(|declaration| matches!(declaration, PropertyDeclaration::FontFamily(_)))
    }

    /// An SVG `transform` attribute (SVG transform-list syntax, e.g. unitless
    /// `translate(20, 30)`) as a CSS `transform` declaration. Translations are
    /// written as `px` lengths so that `zoom` scales them as it would CSS ones.
    #[cfg(feature = "svg")]
    fn parse_svg_transform_attr(value: &str) -> Option<PropertyDeclaration> {
        use std::fmt::Write;
        use svgtypes::{TransformListParser, TransformListToken as Token};

        let mut css = String::new();
        for token in TransformListParser::from(value) {
            match token.ok()? {
                Token::Matrix { a, b, c, d, e, f } => {
                    write!(
                        css,
                        "translate({e}px, {f}px) matrix({a}, {b}, {c}, {d}, 0, 0) "
                    )
                }
                Token::Translate { tx, ty } => write!(css, "translate({tx}px, {ty}px) "),
                Token::Scale { sx, sy } => write!(css, "scale({sx}, {sy}) "),
                Token::Rotate { angle } => write!(css, "rotate({angle}deg) "),
                Token::SkewX { angle } => write!(css, "skewX({angle}deg) "),
                Token::SkewY { angle } => write!(css, "skewY({angle}deg) "),
            }
            .ok()?;
        }
        if css.is_empty() {
            return None;
        }
        parse_declaration("transform", &css)
    }

    // The outermost `<svg>` is a CSS box, so its `transform` attribute transforms
    // the box like the CSS `transform` property it maps to.
    // https://svgwg.org/svg2-draft/coords.html#TransformProperty
    #[cfg(feature = "svg")]
    let is_outermost_svg = *tag == local_name!("svg")
        && elem.name.ns == ns!(svg)
        && node
            .parent_node()
            .and_then(|parent| parent.data.downcast_element())
            .is_none_or(|parent| parent.name.ns != ns!(svg));

    // `<input type=image>` is the only input type that is replaced
    // content, and it is the only one that takes the embedded-content
    // presentational attributes. The type attribute is matched ASCII
    // case-insensitively, as attribute keywords always are.
    let is_image_input = *tag == local_name!("input")
        && elem.attrs().iter().any(|attr| {
            attr.name.local == local_name!("type") && attr.value.eq_ignore_ascii_case("image")
        });

    // An `hr` with `color` or `noshade` takes its `size` as border widths
    // rather than as a height.
    let is_solid_hr = *tag == local_name!("hr")
        && (elem.has_attr(local_name!("color")) || elem.has_attr(local_name!("noshade")));

    // https://html.spec.whatwg.org/multipage/#the-lang-and-xml:lang-attributes
    let accepts_lang = elem.name.ns == ns!(html) || elem.name.ns == ns!(svg);
    let mut lang = None;

    #[cfg(feature = "svg")]
    let svg_url_data =
        (elem.name.ns == ns!(svg)).then(|| crate::url::DocumentUrl::default().url_extra_data());
    // Stack buffer large enough for one declaration (or one expanded
    // shorthand), reused for every presentation attribute on the element.
    #[cfg(feature = "svg")]
    let mut svg_hint = style::properties::SourcePropertyDeclaration::default();

    // `dir=auto`, and a `bdi` with no valid `dir`, take their direction
    // from the first strong character of their text, as HTML's bidi
    // rendering maps them; with none, the inherited direction stands.
    let dir = elem
        .attrs()
        .iter()
        .find(|attr| attr.name.local == local_name!("dir"))
        .map(|attr| attr.value.as_str());
    let dir_is = |keyword: &str| dir.is_some_and(|dir| dir.eq_ignore_ascii_case(keyword));
    let dir_auto = dir_is("auto")
        || (*tag == local_name!("bdi")
            && elem.name.ns == ns!(html)
            && !dir_is("ltr")
            && !dir_is("rtl"));
    if dir_auto {
        if let Some(rtl) = node.auto_direction_is_rtl() {
            use style::properties::longhands::direction::SpecifiedValue as Direction;
            hints.push(PropertyDeclaration::Direction(if rtl {
                Direction::Rtl
            } else {
                Direction::Ltr
            }));
        }
    }

    // A list box shows as many rows as its display size: the `size` attribute, or 4 for a
    // `multiple` select without one (https://html.spec.whatwg.org/#concept-select-size).
    // The user-agent sheet sets each option 1.2em tall.
    if *tag == local_name!("select") {
        let size = elem
            .attr(local_name!("size"))
            .and_then(|size| size.trim().parse::<u32>().ok())
            .filter(|size| *size > 0);
        let rows = if elem.has_attr(local_name!("multiple")) {
            Some(size.unwrap_or(4))
        } else {
            size.filter(|size| *size > 1)
        };
        if let Some(rows) = rows {
            use style::values::generics::{NonNegative, length::Size};
            use style::values::specified::{LengthPercentage, NoCalcLength};
            let rows = rows.min(10_000) as f32;
            let length = NoCalcLength::from_em(rows * 1.2);
            let size = Size::LengthPercentage(NonNegative(LengthPercentage::Length(length)));
            hints.push(PropertyDeclaration::BlockSize(size));
        }
    }

    for attr in elem.attrs() {
        let name = &attr.name.local;
        let value = attr.value.as_str();

        if *name == local_name!("lang") {
            if attr.name.ns == ns!(xml) {
                lang = Some(value);
            } else if attr.name.ns == ns!() && accepts_lang {
                lang = lang.or(Some(value));
            }
        }

        // A `font` element's `face` sets `font-family` to the attribute's value, and
        // its `size` maps to a font size keyword.
        // https://html.spec.whatwg.org/multipage/rendering.html#phrasing-content-3
        if *tag == local_name!("font") && elem.name.ns == ns!(html) {
            if *name == local_name!("face")
                && let Some(family) = parse_font_face_attr(value)
            {
                hints.push(family);
            } else if *name == local_name!("size")
                && let Some(size) = parse_legacy_font_size(value)
            {
                hints.push(PropertyDeclaration::FontSize(
                    style::values::specified::FontSize::from_html_size(size),
                ));
            }
        }

        if *name == local_name!("align") {
            use style::values::specified::TextAlign;
            let keyword = match value {
                "left" => Some(StyloTextAlign::MozLeft),
                "right" => Some(StyloTextAlign::MozRight),
                "center" => Some(StyloTextAlign::MozCenter),
                _ => None,
            };

            if let Some(keyword) = keyword {
                hints.push(PropertyDeclaration::TextAlign(TextAlign::Keyword(keyword)));
            }
        }

        // `valign` on HTML table row groups, rows and cells maps to
        // `vertical-align`, matched ASCII case-insensitively. `vertical-align`
        // is a shorthand in Stylo, so push the three longhands each keyword
        // expands to (as `shorthands::vertical_align::parse_value` does).
        // https://html.spec.whatwg.org/multipage/rendering.html#tables-2
        if *name == local_name!("valign")
            && elem.name.ns == ns!(html)
            && matches_tag!(tag, "thead" | "tbody" | "tfoot" | "tr" | "td" | "th")
        {
            use style::values::generics::box_::BaselineShiftKeyword;
            use style::values::specified::{AlignmentBaseline, BaselineShift, BaselineSource};
            let longhands = if value.eq_ignore_ascii_case("top") {
                Some((
                    AlignmentBaseline::Baseline,
                    BaselineShift::Keyword(BaselineShiftKeyword::Top),
                ))
            } else if value.eq_ignore_ascii_case("middle") {
                Some((AlignmentBaseline::Middle, BaselineShift::zero()))
            } else if value.eq_ignore_ascii_case("bottom") {
                Some((
                    AlignmentBaseline::Baseline,
                    BaselineShift::Keyword(BaselineShiftKeyword::Bottom),
                ))
            } else if value.eq_ignore_ascii_case("baseline") {
                Some((AlignmentBaseline::Baseline, BaselineShift::zero()))
            } else {
                None
            };

            if let Some((alignment_baseline, baseline_shift)) = longhands {
                hints.push(PropertyDeclaration::BaselineSource(BaselineSource::Auto));
                hints.push(PropertyDeclaration::AlignmentBaseline(alignment_baseline));
                hints.push(PropertyDeclaration::BaselineShift(baseline_shift));
            }
        }

        // The width/height attributes on these elements map to the
        // corresponding dimension properties (percentages allowed):
        // https://html.spec.whatwg.org/multipage/rendering.html#dimRendering
        // https://html.spec.whatwg.org/multipage/rendering.html#attributes-for-embedded-content-and-images
        let is_width = *name == local_name!("width");
        let is_height = *name == local_name!("height");
        // The elements whose width/height attributes map to the dimension
        // properties. `input` only joins them as type=image, which is the
        // one replaced input type.
        let is_embedded = is_image_input
            || matches_tag!(
                tag,
                "iframe" | "embed" | "video" | "object" | "img" | "marquee"
            );
        let maps_to_dimension = if is_width {
            is_embedded
                || matches_tag!(
                    tag,
                    "table" | "col" | "colgroup" | "tr" | "td" | "th" | "hr"
                )
        } else if is_height {
            is_embedded
                || matches_tag!(
                    tag,
                    "table" | "thead" | "tbody" | "tfoot" | "tr" | "td" | "th"
                )
        } else {
            false
        };
        if maps_to_dimension {
            // Three of these use the "(ignoring zero)" variant of the
            // mapping, where a zero is dropped instead of honoured:
            // `table width`, and `td`/`th` in both axes.
            let is_cell = matches_tag!(tag, "td" | "th");
            let ignoring_zero = is_cell || (is_width && *tag == local_name!("table"));
            if let Some(length) = parse_size_attr(value, ignoring_zero) {
                hints.push(if is_width {
                    PropertyDeclaration::Width(size(length))
                } else {
                    PropertyDeclaration::Height(size(length))
                });
            }
        }

        // hspace/vspace map to the horizontal and vertical margins as
        // dimension properties. This is a *smaller* set than the one that
        // takes width/height: `iframe` and `video` take width and height
        // but not hspace/vspace, and html/rendering/unmapped-attributes
        // checks exactly that -- browsers have got it wrong before.
        let takes_spacing =
            is_image_input || matches_tag!(tag, "embed" | "img" | "object" | "marquee");
        if takes_spacing
            && matches_tag!(name, "hspace" | "vspace")
            && let Some(length) = parse_size_attr(value, false)
        {
            let margin = Margin::LengthPercentage(length);
            if *name == local_name!("hspace") {
                hints.extend(apply_to_all!(margin => MarginLeft, MarginRight));
            } else {
                hints.extend(apply_to_all!(margin => MarginTop, MarginBottom));
            }
        }

        // `hr`'s `color` maps to `color` as a legacy colour value, and its
        // `size` to the border widths (with `color` or `noshade` present)
        // or to the height. The solid border style that `color` and
        // `noshade` also ask for is a UA rule in default.css.
        // https://html.spec.whatwg.org/multipage/rendering.html#the-hr-element-2
        if *tag == local_name!("hr") {
            if *name == local_name!("color") {
                use style::values::specified::{Color, ColorPropertyValue};
                if let Ok(color) = style::servo::attr::parse_legacy_color(value) {
                    hints.push(PropertyDeclaration::Color(ColorPropertyValue(
                        Color::from_absolute_color(color),
                    )));
                }
            }
            if *name == local_name!("size")
                && let Some(hr_size) = parse_pixels(value)
            {
                if is_solid_hr {
                    let width = BorderSideWidth::from_px(hr_size as f32 / 2.0);
                    hints.extend(apply_to_all!(width =>
                        BorderTopWidth, BorderRightWidth, BorderBottomWidth, BorderLeftWidth,
                    ));
                } else if hr_size == 1 {
                    hints.push(PropertyDeclaration::BorderBottomWidth(
                        BorderSideWidth::from_px(0.0),
                    ));
                } else if hr_size > 1 {
                    hints.push(PropertyDeclaration::Height(size(px((hr_size - 2) as f32))));
                }
            }
        }

        // https://svgwg.org/svg2-draft/geometry.html#Sizing
        // The `width` and `height` attributes on an `<svg>` element are
        // presentation attributes that map to the CSS `width`/`height`
        // properties, so e.g. `width="1em"` must resolve against the
        // element's font-size like any other CSS length.
        let is_svg_root_size = *tag == local_name!("svg")
            && (*name == local_name!("width") || *name == local_name!("height"));
        if is_svg_root_size {
            if let Some(length) = parse_svg_size_attr(value) {
                hints.push(if *name == local_name!("width") {
                    PropertyDeclaration::Width(size(length))
                } else {
                    PropertyDeclaration::Height(size(length))
                });
            }
        }

        // usvg paints the outermost `<svg>`'s `transform`, `opacity`, `filter`,
        // `clip-path` and `mask` attributes itself, so mapping them onto its
        // CSS box as well would apply them twice. Its `transform` is still
        // mapped below, for the box: the serialized source drops it.
        #[cfg(feature = "svg")]
        if let Some(url_data) = &svg_url_data
            && attr.name.ns == ns!()
            && !is_svg_root_size
            && !(is_outermost_svg
                && matches!(
                    name.as_ref(),
                    "transform" | "opacity" | "filter" | "clip-path" | "mask"
                ))
        {
            super::svg_attrs::svg_presentation_hint(tag, name, value, url_data, &mut svg_hint);
            hints.extend(svg_hint.drain().declarations);
        }

        #[cfg(feature = "svg")]
        if is_outermost_svg
            && attr.name.ns == ns!()
            && *name == local_name!("transform")
            && let Some(transform) = parse_svg_transform_attr(value)
        {
            hints.push(transform);
        }

        // The `border` attribute maps to the four border widths as a
        // pixel length, plus the four border styles as `solid` -- width
        // alone would compute back to zero against the default
        // border-style of `none`. It is only these three elements:
        // `embed`, `iframe`, `marquee` and non-image `input` all have a
        // `border` attribute that must stay unmapped.
        if *name == local_name!("border")
            && (is_image_input || matches_tag!(tag, "img" | "object"))
            && let Some(width) = parse_pixels(value)
        {
            use style::values::specified::BorderStyle;
            hints.extend(apply_to_all!(BorderSideWidth::from_px(width as f32) =>
                BorderTopWidth, BorderRightWidth, BorderBottomWidth, BorderLeftWidth,
            ));
            hints.extend(apply_to_all!(BorderStyle::Solid =>
                BorderTopStyle, BorderRightStyle, BorderBottomStyle, BorderLeftStyle,
            ));
        }

        // `body` carries four legacy margin attributes, as pixel lengths:
        // marginwidth and marginheight set both sides of an axis, and
        // leftmargin and topmargin set one side each.
        //
        // There is deliberately no `rightmargin` or `bottommargin`. They
        // look like the obvious counterparts to the two that exist, but
        // the spec does not define them and browsers ignore them in both
        // standards and quirks mode -- body-margin-3a/3b assert exactly
        // that.
        if *tag == local_name!("body") {
            // Matched as strings: these are not in the static atom set,
            // so `local_name!` will not compile for them.
            let margin =
                || parse_pixels(value).map(|margin| Margin::LengthPercentage(px(margin as f32)));
            match &**name {
                "marginwidth" => {
                    if let Some(margin) = margin() {
                        hints.extend(apply_to_all!(margin => MarginLeft, MarginRight));
                    }
                }
                "marginheight" => {
                    if let Some(margin) = margin() {
                        hints.extend(apply_to_all!(margin => MarginTop, MarginBottom));
                    }
                }
                "leftmargin" => {
                    if let Some(margin) = margin() {
                        hints.push(PropertyDeclaration::MarginLeft(margin));
                    }
                }
                "topmargin" => {
                    if let Some(margin) = margin() {
                        hints.push(PropertyDeclaration::MarginTop(margin));
                    }
                }
                _ => {}
            }
        }

        // https://html.spec.whatwg.org/multipage/rendering.html#tables-2
        if *name == local_name!("cellspacing")
            && *tag == local_name!("table")
            && let Some(spacing) = parse_pixels(value)
        {
            use style::values::specified::{BorderSpacing, NonNegativeLength};
            let spacing = NonNegativeLength::from_px(spacing as f32);
            hints.push(PropertyDeclaration::BorderSpacing(BorderSpacing::new(
                spacing.clone(),
                spacing,
            )));
        }

        if *name == local_name!("bgcolor") {
            use style::values::specified::Color;
            if let Some((r, g, b, a)) = parse_color_attr(value) {
                hints.push(PropertyDeclaration::BackgroundColor(
                    Color::from_absolute_color(AbsoluteColor::srgb_legacy(r, g, b, a)),
                ));
            }
        }

        if *name == local_name!("hidden") {
            use style::values::specified::Display;
            hints.push(PropertyDeclaration::Display(Display::None));
        }
    }

    if let Some(lang) = lang {
        hints.push(PropertyDeclaration::XLang(XLang(Atom::from(lang))));
    }

    let wrap = |block: PropertyDeclarationBlock| {
        (!block.is_empty()).then(|| ServoArc::new(node.guard().wrap(block)))
    };

    // A table's `cellpadding` attribute maps to the padding of its cells (not
    // those of nested tables), as a pixel length.
    // https://html.spec.whatwg.org/multipage/rendering.html#tables-2
    let cell_padding = (*tag == local_name!("table"))
        .then(|| elem.attr(local_name!("cellpadding")))
        .flatten()
        .and_then(parse_pixels)
        .and_then(|padding| {
            let mut block = Hints(PropertyDeclarationBlock::new());
            block.extend(apply_to_all!(NonNegative(px(padding as f32)) =>
                PaddingTop, PaddingRight, PaddingBottom, PaddingLeft,
            ));
            wrap(block.0)
        });

    PresentationalHints {
        own: wrap(hints.0),
        cell_padding,
    }
}
