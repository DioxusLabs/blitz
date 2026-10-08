//! Conversion from Stylo computed values to winkin's styles.
//!
//! Every length is in device pixels, which is what Blitz lays text out in.
//! A percentage of the containing block is resolved against `basis`, its
//! inline size in CSS pixels, as the cascade would have were it to know it;
//! one only layout can resolve -- a `word-spacing` percentage of the space it
//! widens, a `text-indent` percentage of the line -- is passed on as a
//! fraction.
//!
//! What Stylo does not compute stays at winkin's initial value:
//! `white-space-trim`, `text-emphasis-skip`, `line-padding`,
//! `line-fit-edge`, `initial-letter-align` and `text-group-align`.

use std::borrow::Cow;
use std::cell::Cell;

use style::Atom;
use style::computed_values::box_decoration_break::T as StyloBoxDecorationBreak;
use style::computed_values::direction::T as StyloDirection;
use style::computed_values::font_kerning::T as StyloFontKerning;
use style::computed_values::font_optical_sizing::T as StyloFontOpticalSizing;
use style::computed_values::font_variant_caps::T as StyloFontVariantCaps;
use style::computed_values::font_variant_emoji::T as StyloFontVariantEmoji;
use style::computed_values::font_variant_position::T as StyloFontVariantPosition;
use style::computed_values::hyphens::T as StyloHyphens;
use style::computed_values::ruby_align::T as StyloRubyAlign;
use style::computed_values::ruby_overhang::T as StyloRubyOverhang;
use style::computed_values::text_orientation::T as StyloTextOrientation;
use style::computed_values::text_spacing_trim::T as StyloTextSpacingTrim;
use style::computed_values::text_wrap_mode::T as StyloTextWrapMode;
use style::computed_values::text_wrap_style::T as StyloTextWrapStyle;
use style::computed_values::unicode_bidi::T as StyloUnicodeBidi;
use style::computed_values::white_space_collapse::T as StyloWhiteSpace;
use style::logical_geometry::WritingMode as StyloWritingMode;
use style::properties::ComputedValues;
use style::servo_arc::Arc as ServoArc;
use style::stylesheets::FontFeatureValuesRule;
use style::stylesheets::font_feature_values_rule::{FFVDeclaration, PairValues, SingleValue};
use style::stylist::Stylist;
use style::values::CustomIdent;
use style::values::computed::font::{
    FontStyle as StyloFontStyle, GenericFontFamily as StyloGenericFontFamily,
};
use style::values::computed::font::{
    FontSynthesis as StyloFontSynthesis, FontSynthesisStyle as StyloFontSynthesisStyle,
    FontVariantEastAsian, FontVariantLigatures, FontVariantNumeric, LineHeight as StyloLineHeight,
    SingleFontFamily,
};
use style::values::computed::length_percentage::Unpacked;
use style::values::computed::text::TextAlignLast as StyloTextAlignLast;
use style::values::computed::text::TextEmphasisStyle as StyloTextEmphasisStyle;
use style::values::computed::{
    BorderSideWidth, Length, LengthPercentage as StyloLengthPercentage,
    TextTransform as StyloTextTransform,
};
use style::values::computed::{
    HangingPunctuation as StyloHangingPunctuation, TextCombineUpright as StyloTextCombineUpright,
};
use style::values::generics::box_::{BaselineShiftKeyword, GenericBaselineShift};
use style::values::generics::font::GenericFontSizeAdjust;
use style::values::generics::length::GenericLengthOrNumber as StyloLengthOrNumber;
use style::values::generics::length::GenericMargin;
use style::values::generics::text::InitialLetterSink as StyloInitialLetterSink;
use style::values::specified::HyphenateCharacter;
use style::values::specified::TextAlignKeyword;
use style::values::specified::border::BorderStyle;
use style::values::specified::box_::{
    AlignmentBaseline, DominantBaseline as StyloDominantBaseline,
};
use style::values::specified::font::VariantAlternates;
use style::values::specified::text::{
    RubyPosition as StyloRubyPosition, TextAutospace as StyloTextAutospace,
    TextBoxEdge as StyloTextBoxEdge, TextBoxTrim as StyloTextBoxTrim, TextDecorationLine,
    TextEdgeKeyword, TextEmphasisPosition as StyloTextEmphasisPosition,
    TextJustify as StyloTextJustify, TextOverflowSide as StyloTextOverflowSide,
};
use winkin::style::{
    AdjustMetric, BaseDirection, BidiGroup, BoxDecorationBreak, Direction, DominantBaseline,
    EdgesGroup, EmphasisPosition, EmphasisSide, EmphasisSkip, EmphasisVerticalSide, FontFamilyName,
    FontFeature, FontGroup, FontKerning, FontLanguageOverride, FontOpticalSizing, FontSizeAdjust,
    FontSynthesis, FontVariantCaps, FontVariantEmoji, FontVariantPosition, FontVariants,
    FontVariation, HangEnd, HangingPunctuation, Hyphens, InitialLetter, InitialLetterAlign,
    Language, LengthPercentage, LineBreak, LineClamp, LineGroup, LineHeight, OrientationGroup,
    OverflowWrap, RubyAlign, RubyGroup, RubyOverhang, RubyPosition, Sides, TabSize, Tag, TextAlign,
    TextAlignLast, TextAutospace, TextBoxEdge, TextBoxTrim, TextCase, TextCombineUpright,
    TextEdgeOver, TextEdgeUnder, TextEmphasis, TextGroup, TextIndent, TextJustify, TextOrientation,
    TextOverflow, TextSpacingTrim, TextTransform, TextWrapMode, TextWrapStyle, UnicodeBidi,
    VerticalAlign, WhiteSpaceCollapse, WordBreak, WritingMode,
};
use winkin::style::{FontStyle, FontWeight, FontWidth, GenericFamily};
use winkin::{ComputedBlockStyle, ComputedStyle};

/// The document's `@font-feature-values` rules, which name the values
/// `font-variant-alternates` asks for, the least precedent first.
#[derive(Default)]
pub(crate) struct FeatureValues(Vec<ServoArc<FontFeatureValuesRule>>);

impl FeatureValues {
    /// The rules in effect in `stylist`, in cascade order: by origin, then
    /// by layer.
    pub(crate) fn of(stylist: &Stylist) -> Self {
        Self(
            stylist
                .iter_extra_data_origins_rev()
                .flat_map(|(data, _)| data.font_feature_values.iter())
                .map(|(rule, _)| rule.clone())
                .collect(),
        )
    }

    /// Adds the features `computed`'s `font-variant-alternates` resolves to.
    ///
    /// Chrome resolves a name against the rules for the family a font is
    /// taken from. Here it is the first family in `font-family` that any
    /// rule names, and a name no rule for it defines adds nothing.
    fn resolve(&self, computed: &ComputedValues, out: &mut Vec<FontFeature>) {
        if self.0.is_empty() {
            return;
        }
        let alternates = computed.clone_font_variant_alternates();
        if alternates
            .iter()
            .all(|alternate| matches!(alternate, VariantAlternates::HistoricalForms))
        {
            return;
        }
        let names_family = |rule: &FontFeatureValuesRule, family: &str| {
            rule.family_names
                .iter()
                .any(|name| name.name.as_str().eq_ignore_ascii_case(family))
        };
        let Some(family) = computed
            .get_font()
            .font_family
            .families
            .list
            .iter()
            .filter_map(|family| match family {
                SingleFontFamily::FamilyName(name) => Some(&*name.name),
                SingleFontFamily::Generic(_) => None,
            })
            .find(|family| self.0.iter().any(|rule| names_family(rule, family)))
        else {
            return;
        };
        // A later rule outweighs an earlier one, name by name.
        let find = |name: &CustomIdent, block: Block| {
            self.0
                .iter()
                .rev()
                .filter(|rule| names_family(rule, family))
                .find_map(|rule| block.find(rule, &name.0))
        };
        let feature = |tag: [u8; 4], value: u32| FontFeature {
            tag: Tag::from_bytes(tag),
            value: value.min(u32::from(u16::MAX)) as u16,
        };
        // `ssNN` and `cvNN`, for a number from 1 to 99.
        let numbered = |prefix: [u8; 2], number: u32| {
            (1..=99).contains(&number).then(|| {
                [
                    prefix[0],
                    prefix[1],
                    b'0' + (number / 10) as u8,
                    b'0' + (number % 10) as u8,
                ]
            })
        };
        for alternate in alternates.iter() {
            match alternate {
                VariantAlternates::Stylistic(name) => {
                    if let Some(value) = find(name, Block::Stylistic) {
                        out.push(feature(*b"salt", value[0]));
                    }
                }
                VariantAlternates::Swash(name) => {
                    if let Some(value) = find(name, Block::Swash) {
                        out.push(feature(*b"swsh", value[0]));
                        out.push(feature(*b"cswh", value[0]));
                    }
                }
                VariantAlternates::Ornaments(name) => {
                    if let Some(value) = find(name, Block::Ornaments) {
                        out.push(feature(*b"ornm", value[0]));
                    }
                }
                VariantAlternates::Annotation(name) => {
                    if let Some(value) = find(name, Block::Annotation) {
                        out.push(feature(*b"nalt", value[0]));
                    }
                }
                VariantAlternates::Styleset(names) => {
                    for name in names.iter() {
                        for set in find(name, Block::Styleset).unwrap_or_default() {
                            if let Some(tag) = numbered(*b"ss", set) {
                                out.push(feature(tag, 1));
                            }
                        }
                    }
                }
                VariantAlternates::CharacterVariant(names) => {
                    for name in names.iter() {
                        let Some(variant) = find(name, Block::CharacterVariant) else {
                            continue;
                        };
                        if let Some(tag) = numbered(*b"cv", variant[0]) {
                            out.push(feature(tag, variant.get(1).copied().unwrap_or(1)));
                        }
                    }
                }
                VariantAlternates::HistoricalForms => {}
            }
        }
    }
}

/// A block of an `@font-feature-values` rule, which one function of
/// `font-variant-alternates` reads.
#[derive(Copy, Clone)]
enum Block {
    Stylistic,
    Swash,
    Ornaments,
    Annotation,
    Styleset,
    CharacterVariant,
}

impl Block {
    /// The values `rule` gives `name` in this block, where it gives any.
    fn find(self, rule: &FontFeatureValuesRule, name: &Atom) -> Option<Vec<u32>> {
        let single = |declarations: &[FFVDeclaration<SingleValue>]| {
            declarations
                .iter()
                .find(|declaration| declaration.name == *name)
                .map(|declaration| vec![declaration.value.0])
        };
        match self {
            Self::Stylistic => single(&rule.stylistic),
            Self::Swash => single(&rule.swash),
            Self::Ornaments => single(&rule.ornaments),
            Self::Annotation => single(&rule.annotation),
            Self::Styleset => rule
                .styleset
                .iter()
                .find(|declaration| declaration.name == *name)
                .map(|declaration| declaration.value.0.clone()),
            Self::CharacterVariant => rule
                .character_variant
                .iter()
                .find(|declaration| declaration.name == *name)
                .map(|declaration| {
                    let PairValues(first, second) = declaration.value;
                    std::iter::once(first).chain(second).collect()
                }),
        }
    }
}

/// The lists an element's style borrows: its families, and the features
/// and variations it asks for.
///
/// The features are what `font-variant-alternates` names through
/// `@font-feature-values`, then `font-feature-settings`, as Chrome orders
/// them: the other `font-variant-*` properties are winkin's to turn into
/// features, and to synthesize where the font has none.
pub(crate) struct FontLists<'a> {
    families: Vec<FontFamilyName<'a>>,
    features: Vec<FontFeature>,
    variations: Vec<FontVariation>,
}

impl<'a> FontLists<'a> {
    /// The lists `computed` asks for, borrowing its family names.
    pub(crate) fn of(computed: &'a ComputedValues, values: &FeatureValues) -> Self {
        let font = computed.get_font();
        let families = font
            .font_family
            .families
            .list
            .iter()
            .map(font_family_name)
            .collect();
        let mut features = Vec::new();
        values.resolve(computed, &mut features);
        features.extend(font.font_feature_settings.0.iter().map(|setting| {
            FontFeature::new(
                Tag::from_bytes(setting.tag.0.to_be_bytes()),
                setting.value as u16,
            )
        }));
        Self {
            families,
            features,
            variations: font
                .font_variation_settings
                .0
                .iter()
                .map(|setting| FontVariation {
                    tag: Tag::from_bytes(setting.tag.0.to_be_bytes()),
                    value: setting.value,
                })
                .collect(),
        }
    }

    /// These lists with `families` tried first, as a list marker's bullet
    /// font is.
    pub(crate) fn with_families_first(mut self, first: &[FontFamilyName<'static>]) -> Self {
        let rest = std::mem::take(&mut self.families);
        self.families = first.iter().cloned().chain(rest).collect();
        self
    }
}

/// A family of `font-family`, as fontwich names it.
pub(crate) fn font_family_name(family: &SingleFontFamily) -> FontFamilyName<'_> {
    match family {
        SingleFontFamily::FamilyName(name) => family_name(name.name.as_str()),
        SingleFontFamily::Generic(generic) => FontFamilyName::Generic(generic_family(*generic)),
    }
}

/// A named family, as Blitz names it to Parley: the Apple system font's
/// legacy names are the `system-ui` generic.
fn family_name(name: &str) -> FontFamilyName<'_> {
    #[cfg(target_vendor = "apple")]
    if name == "-apple-system" || name == "BlinkMacSystemFont" {
        return FontFamilyName::Generic(winkin::style::GenericFamily::SystemUi);
    }
    FontFamilyName::Named(Cow::Borrowed(name))
}

/// A length-percentage as a length in device pixels and a fraction of
/// whatever layout resolves it against.
///
/// `calc()` mixing the two is split by resolving it against two bases: the
/// value is linear in its basis.
fn length_percentage(value: &StyloLengthPercentage, scale: f32) -> LengthPercentage {
    let at_zero = value.resolve(Length::new(0.0)).px();
    let at_hundred = value.resolve(Length::new(100.0)).px();
    LengthPercentage {
        px: at_zero * scale,
        fraction: (at_hundred - at_zero) / 100.0,
    }
}

/// The containing block's inline size, in CSS pixels, that percentages of it
/// are resolved against, and whether any was.
pub(crate) struct Basis {
    px: f32,
    read: Cell<bool>,
}

impl Basis {
    /// A basis `px` CSS pixels wide, which nothing has read yet.
    pub(crate) fn new(px: f32) -> Self {
        Self {
            px,
            read: Cell::new(false),
        }
    }

    /// Its size in CSS pixels.
    pub(crate) fn px(&self) -> f32 {
        self.px
    }

    /// Whether a percentage was resolved against it.
    pub(crate) fn is_read(&self) -> bool {
        self.read.get()
    }
}

/// A length-percentage of the containing block, `basis` wide, in device
/// pixels.
fn of_basis(value: &StyloLengthPercentage, basis: &Basis, scale: f32) -> f32 {
    if value.has_percentage() {
        basis.read.set(true);
    }
    value.resolve(Length::new(basis.px)).px() * scale
}

/// Converts an element's computed style, its lists borrowed from `lists`.
///
/// Lengths are scaled by `scale` into device pixels, and percentages of the
/// containing block resolved against `basis`. `language` is the
/// content language from the nearest `lang`.
pub(crate) fn computed_style<'a>(
    lists: &'a FontLists<'_>,
    computed: &'a ComputedValues,
    scale: f32,
    basis: &Basis,
    language: Option<Language>,
) -> ComputedStyle<'a> {
    ComputedStyle {
        font: font_group(computed, lists, scale),
        text: text_group(computed, scale, language),
        line: line_group(computed, scale),
        bidi: BidiGroup {
            direction: if computed.get_inherited_box().direction == StyloDirection::Rtl {
                Direction::Rtl
            } else {
                Direction::Ltr
            },
            unicode_bidi: unicode_bidi(computed),
        },
        orientation: OrientationGroup {
            // Which way each character stands in a vertical line. It
            // inherits, so a span may set it against the block around it.
            text_orientation: match computed.clone_text_orientation() {
                StyloTextOrientation::Mixed => TextOrientation::Mixed,
                StyloTextOrientation::Upright => TextOrientation::Upright,
                StyloTextOrientation::Sideways => TextOrientation::Sideways,
            },
            text_combine_upright: match computed.clone_text_combine_upright() {
                StyloTextCombineUpright::None => TextCombineUpright::None,
                StyloTextCombineUpright::All => TextCombineUpright::All,
                StyloTextCombineUpright::Digits(count) => TextCombineUpright::Digits(count),
            },
        },
        edges: edges_group(computed, scale, basis),
        ruby: RubyGroup {
            position: ruby_position(computed),
            align: match computed.clone_ruby_align() {
                StyloRubyAlign::SpaceAround => RubyAlign::SpaceAround,
                StyloRubyAlign::SpaceBetween => RubyAlign::SpaceBetween,
                StyloRubyAlign::Center => RubyAlign::Center,
                StyloRubyAlign::Start => RubyAlign::Start,
            },
            overhang: match computed.clone_ruby_overhang() {
                StyloRubyOverhang::Auto => RubyOverhang::Auto,
                StyloRubyOverhang::Spaces => RubyOverhang::Spaces,
            },
        },
        paints: paints(computed),
        decorates: decorates(computed),
    }
}

/// `unicode-bidi`.
pub(crate) fn unicode_bidi(computed: &ComputedValues) -> UnicodeBidi {
    match computed.clone_unicode_bidi() {
        StyloUnicodeBidi::Normal => UnicodeBidi::Normal,
        StyloUnicodeBidi::Embed => UnicodeBidi::Embed,
        StyloUnicodeBidi::Isolate => UnicodeBidi::Isolate,
        StyloUnicodeBidi::BidiOverride => UnicodeBidi::BidiOverride,
        StyloUnicodeBidi::IsolateOverride => UnicodeBidi::IsolateOverride,
        StyloUnicodeBidi::Plaintext => UnicodeBidi::Plaintext,
    }
}

/// Whether the box draws something of its own -- a background, a border,
/// an outline -- which is what keeps its fragment in the layout. A
/// decoration line doesn't count. What it draws is read from the node when
/// painting.
fn paints(computed: &ComputedValues) -> bool {
    let current_color = computed.clone_color();
    let background = computed.get_background();
    let fills = background
        .background_color
        .resolve_to_absolute(&current_color)
        .alpha
        > 0.0;
    let images = background
        .background_image
        .0
        .iter()
        .any(|image| !matches!(image, style::values::computed::Image::None));
    let border = computed.get_border();
    let bordered = [
        (&border.border_top_width, border.border_top_style),
        (&border.border_right_width, border.border_right_style),
        (&border.border_bottom_width, border.border_bottom_style),
        (&border.border_left_width, border.border_left_style),
    ]
    .into_iter()
    .any(|(width, style)| !style.none_or_hidden() && width.0.to_f32_px() > 0.0);
    let outline = computed.get_outline();
    let outlined =
        !outline.outline_style.none_or_hidden() && outline.outline_width.0.to_f32_px() > 0.0;
    fills || images || bordered || outlined
}

/// Whether the element sets a decoration line that is drawn.
pub(crate) fn decorates(computed: &ComputedValues) -> bool {
    computed.clone_text_decoration_line().intersects(
        TextDecorationLine::UNDERLINE
            | TextDecorationLine::OVERLINE
            | TextDecorationLine::LINE_THROUGH,
    )
}

fn font_group<'a>(
    computed: &ComputedValues,
    lists: &'a FontLists<'_>,
    scale: f32,
) -> FontGroup<'a> {
    let font = computed.get_font();
    let synthesis = |value: StyloFontSynthesis| value == StyloFontSynthesis::Auto;
    FontGroup {
        families: &lists.families,
        features: &lists.features,
        variations: &lists.variations,
        size: font.font_size.used_size.0.px() * scale,
        weight: FontWeight::new(font.font_weight.value()),
        width: FontWidth::from_percentage(font.font_width.0.to_float()),
        style: match font.font_style {
            StyloFontStyle::NORMAL => FontStyle::Normal,
            StyloFontStyle::ITALIC => FontStyle::Italic,
            oblique => FontStyle::Oblique(Some(oblique.oblique_degrees())),
        },
        synthesis: FontSynthesis {
            weight: synthesis(computed.clone_font_synthesis_weight()),
            // `oblique-only` allows a slant only where one was asked for.
            style: match computed.clone_font_synthesis_style() {
                StyloFontSynthesisStyle::Auto => true,
                StyloFontSynthesisStyle::None => false,
                StyloFontSynthesisStyle::ObliqueOnly => !matches!(
                    font.font_style,
                    style::values::computed::font::FontStyle::NORMAL
                        | style::values::computed::font::FontStyle::ITALIC
                ),
            },
            small_caps: synthesis(computed.clone_font_synthesis_small_caps()),
            position: synthesis(computed.clone_font_synthesis_position()),
        },
        variant_caps: match computed.clone_font_variant_caps() {
            StyloFontVariantCaps::Normal => FontVariantCaps::Normal,
            StyloFontVariantCaps::SmallCaps => FontVariantCaps::SmallCaps,
            StyloFontVariantCaps::AllSmallCaps => FontVariantCaps::AllSmallCaps,
            StyloFontVariantCaps::PetiteCaps => FontVariantCaps::PetiteCaps,
            StyloFontVariantCaps::AllPetiteCaps => FontVariantCaps::AllPetiteCaps,
            StyloFontVariantCaps::Unicase => FontVariantCaps::Unicase,
            StyloFontVariantCaps::TitlingCaps => FontVariantCaps::TitlingCaps,
        },
        variant_position: match computed.clone_font_variant_position() {
            StyloFontVariantPosition::Normal => FontVariantPosition::Normal,
            StyloFontVariantPosition::Sub => FontVariantPosition::Sub,
            StyloFontVariantPosition::Super => FontVariantPosition::Super,
        },
        variants: variants(computed),
        variant_emoji: match computed.clone_font_variant_emoji() {
            StyloFontVariantEmoji::Normal => FontVariantEmoji::Normal,
            StyloFontVariantEmoji::Text => FontVariantEmoji::Text,
            StyloFontVariantEmoji::Emoji => FontVariantEmoji::Emoji,
            StyloFontVariantEmoji::Unicode => FontVariantEmoji::Unicode,
        },
        optical_sizing: match computed.clone_font_optical_sizing() {
            StyloFontOpticalSizing::Auto => FontOpticalSizing::Auto,
            StyloFontOpticalSizing::None => FontOpticalSizing::None,
        },
        kerning: match computed.clone_font_kerning() {
            StyloFontKerning::Auto => FontKerning::Auto,
            StyloFontKerning::Normal => FontKerning::Normal,
            StyloFontKerning::None => FontKerning::None,
        },
        language_override: match computed.clone_font_language_override().0 {
            0 => FontLanguageOverride::Normal,
            tag => FontLanguageOverride::System(Tag::from_bytes(tag.to_be_bytes())),
        },
        size_adjust: match computed.clone_font_size_adjust() {
            GenericFontSizeAdjust::None => FontSizeAdjust::None,
            GenericFontSizeAdjust::ExHeight(value) => adjust(AdjustMetric::ExHeight, value.0),
            GenericFontSizeAdjust::CapHeight(value) => adjust(AdjustMetric::CapHeight, value.0),
            GenericFontSizeAdjust::ChWidth(value) => adjust(AdjustMetric::ChWidth, value.0),
            GenericFontSizeAdjust::IcWidth(value) => adjust(AdjustMetric::IcWidth, value.0),
            GenericFontSizeAdjust::IcHeight(value) => adjust(AdjustMetric::IcHeight, value.0),
        },
    }
}

fn adjust(metric: AdjustMetric, value: f32) -> FontSizeAdjust {
    FontSizeAdjust::Hold { metric, value }
}

/// The keywords of `font-variant-ligatures`, `-numeric`, `-east-asian` and
/// `-alternates` as one set.
fn variants(computed: &ComputedValues) -> FontVariants {
    let mut set = FontVariants::NORMAL;
    let mut add = |on: bool, keyword: FontVariants| {
        if on {
            set = set.union(keyword);
        }
    };
    let ligatures = computed.clone_font_variant_ligatures();
    if ligatures.contains(FontVariantLigatures::NONE) {
        // `none` is every `no-` keyword.
        add(true, FontVariants::NO_COMMON_LIGATURES);
        add(true, FontVariants::NO_DISCRETIONARY_LIGATURES);
        add(true, FontVariants::NO_HISTORICAL_LIGATURES);
        add(true, FontVariants::NO_CONTEXTUAL);
    }
    for (bit, keyword) in [
        (
            FontVariantLigatures::COMMON_LIGATURES,
            FontVariants::COMMON_LIGATURES,
        ),
        (
            FontVariantLigatures::NO_COMMON_LIGATURES,
            FontVariants::NO_COMMON_LIGATURES,
        ),
        (
            FontVariantLigatures::DISCRETIONARY_LIGATURES,
            FontVariants::DISCRETIONARY_LIGATURES,
        ),
        (
            FontVariantLigatures::NO_DISCRETIONARY_LIGATURES,
            FontVariants::NO_DISCRETIONARY_LIGATURES,
        ),
        (
            FontVariantLigatures::HISTORICAL_LIGATURES,
            FontVariants::HISTORICAL_LIGATURES,
        ),
        (
            FontVariantLigatures::NO_HISTORICAL_LIGATURES,
            FontVariants::NO_HISTORICAL_LIGATURES,
        ),
        (FontVariantLigatures::CONTEXTUAL, FontVariants::CONTEXTUAL),
        (
            FontVariantLigatures::NO_CONTEXTUAL,
            FontVariants::NO_CONTEXTUAL,
        ),
    ] {
        add(ligatures.contains(bit), keyword);
    }
    let numeric = computed.clone_font_variant_numeric();
    for (bit, keyword) in [
        (FontVariantNumeric::LINING_NUMS, FontVariants::LINING_NUMS),
        (
            FontVariantNumeric::OLDSTYLE_NUMS,
            FontVariants::OLDSTYLE_NUMS,
        ),
        (
            FontVariantNumeric::PROPORTIONAL_NUMS,
            FontVariants::PROPORTIONAL_NUMS,
        ),
        (FontVariantNumeric::TABULAR_NUMS, FontVariants::TABULAR_NUMS),
        (
            FontVariantNumeric::DIAGONAL_FRACTIONS,
            FontVariants::DIAGONAL_FRACTIONS,
        ),
        (
            FontVariantNumeric::STACKED_FRACTIONS,
            FontVariants::STACKED_FRACTIONS,
        ),
        (FontVariantNumeric::ORDINAL, FontVariants::ORDINAL),
        (FontVariantNumeric::SLASHED_ZERO, FontVariants::SLASHED_ZERO),
    ] {
        add(numeric.contains(bit), keyword);
    }
    let east_asian = computed.clone_font_variant_east_asian();
    for (bit, keyword) in [
        (FontVariantEastAsian::JIS78, FontVariants::JIS78),
        (FontVariantEastAsian::JIS83, FontVariants::JIS83),
        (FontVariantEastAsian::JIS90, FontVariants::JIS90),
        (FontVariantEastAsian::JIS04, FontVariants::JIS04),
        (FontVariantEastAsian::SIMPLIFIED, FontVariants::SIMPLIFIED),
        (FontVariantEastAsian::TRADITIONAL, FontVariants::TRADITIONAL),
        (FontVariantEastAsian::FULL_WIDTH, FontVariants::FULL_WIDTH),
        (
            FontVariantEastAsian::PROPORTIONAL_WIDTH,
            FontVariants::PROPORTIONAL_WIDTH,
        ),
        (FontVariantEastAsian::RUBY, FontVariants::RUBY),
    ] {
        add(east_asian.contains(bit), keyword);
    }
    // The other alternates name `@font-feature-values`, and are features
    // of the font lists.
    let historical = computed
        .clone_font_variant_alternates()
        .iter()
        .any(|alternate| matches!(alternate, VariantAlternates::HistoricalForms));
    add(historical, FontVariants::HISTORICAL_FORMS);
    set
}

fn text_group(computed: &ComputedValues, scale: f32, language: Option<Language>) -> TextGroup<'_> {
    let text = computed.get_inherited_text();
    let css_size = computed.get_font().font_size.used_size.0.px();
    let transform = computed.clone_text_transform();
    TextGroup {
        language,
        hyphenate_character: match &text.hyphenate_character {
            HyphenateCharacter::Auto => None,
            HyphenateCharacter::String(string) => Some(&**string),
        },
        white_space_collapse: match text.white_space_collapse {
            StyloWhiteSpace::Collapse => WhiteSpaceCollapse::Collapse,
            StyloWhiteSpace::Preserve => WhiteSpaceCollapse::Preserve,
            StyloWhiteSpace::PreserveBreaks => WhiteSpaceCollapse::PreserveBreaks,
            StyloWhiteSpace::BreakSpaces => WhiteSpaceCollapse::BreakSpaces,
            StyloWhiteSpace::PreserveSpaces => WhiteSpaceCollapse::PreserveSpaces,
        },
        wrap_mode: match text.text_wrap_mode {
            StyloTextWrapMode::Wrap => TextWrapMode::Wrap,
            StyloTextWrapMode::Nowrap => TextWrapMode::NoWrap,
        },
        transform: TextTransform {
            case: if transform.contains(StyloTextTransform::UPPERCASE) {
                TextCase::Uppercase
            } else if transform.contains(StyloTextTransform::LOWERCASE) {
                TextCase::Lowercase
            } else if transform.contains(StyloTextTransform::CAPITALIZE) {
                TextCase::Capitalize
            } else {
                TextCase::None
            },
            full_width: transform.contains(StyloTextTransform::FULL_WIDTH),
            full_size_kana: transform.contains(StyloTextTransform::FULL_SIZE_KANA),
        },
        word_break: match text.word_break {
            style::values::computed::WordBreak::Normal => WordBreak::Normal,
            style::values::computed::WordBreak::BreakAll => WordBreak::BreakAll,
            style::values::computed::WordBreak::KeepAll => WordBreak::KeepAll,
        },
        line_break: match computed.clone_line_break() {
            style::values::computed::LineBreak::Auto
            | style::values::computed::LineBreak::Normal => LineBreak::Normal,
            style::values::computed::LineBreak::Loose => LineBreak::Loose,
            style::values::computed::LineBreak::Strict => LineBreak::Strict,
            style::values::computed::LineBreak::Anywhere => LineBreak::Anywhere,
        },
        overflow_wrap: match text.overflow_wrap {
            style::values::computed::OverflowWrap::Normal => OverflowWrap::Normal,
            style::values::computed::OverflowWrap::BreakWord => OverflowWrap::BreakWord,
            style::values::computed::OverflowWrap::Anywhere => OverflowWrap::Anywhere,
        },
        hyphens: match computed.clone_hyphens() {
            StyloHyphens::None => Hyphens::None,
            StyloHyphens::Manual => Hyphens::Manual,
            StyloHyphens::Auto => Hyphens::Auto,
        },
        tab_size: match text.tab_size {
            StyloLengthOrNumber::Number(spaces) => TabSize::Spaces(spaces.0),
            StyloLengthOrNumber::Length(length) => TabSize::Px(length.0.px() * scale),
        },
        // A percentage is of the font size.
        letter_spacing: text.letter_spacing.0.resolve(Length::new(css_size)).px() * scale,
        // A percentage is of the font size, which layout takes it of.
        word_spacing: length_percentage(&text.word_spacing, scale),
        autospace: {
            let autospace = computed.clone_text_autospace();
            if autospace.intersects(StyloTextAutospace::NORMAL | StyloTextAutospace::AUTO) {
                TextAutospace::NORMAL
            } else {
                TextAutospace {
                    ideograph_alpha: autospace.contains(StyloTextAutospace::IDEOGRAPH_ALPHA),
                    ideograph_numeric: autospace.contains(StyloTextAutospace::IDEOGRAPH_NUMERIC),
                }
            }
        },
        justify: match text.text_justify {
            StyloTextJustify::Auto => TextJustify::Auto,
            StyloTextJustify::None => TextJustify::None,
            StyloTextJustify::InterWord => TextJustify::InterWord,
            StyloTextJustify::InterCharacter => TextJustify::InterCharacter,
        },
        spacing_trim: match computed.clone_text_spacing_trim() {
            StyloTextSpacingTrim::Normal => TextSpacingTrim::Normal,
            StyloTextSpacingTrim::SpaceAll => TextSpacingTrim::SpaceAll,
            StyloTextSpacingTrim::SpaceFirst => TextSpacingTrim::SpaceFirst,
            StyloTextSpacingTrim::TrimStart => TextSpacingTrim::TrimStart,
        },
        hanging_punctuation: {
            let hanging = computed.clone_hanging_punctuation();
            HangingPunctuation {
                first: hanging.contains(StyloHangingPunctuation::FIRST),
                last: hanging.contains(StyloHangingPunctuation::LAST),
                end: if hanging.contains(StyloHangingPunctuation::FORCE_END) {
                    HangEnd::Force
                } else if hanging.contains(StyloHangingPunctuation::ALLOW_END) {
                    HangEnd::Allow
                } else {
                    HangEnd::None
                },
            }
        },
        emphasis: emphasis(computed),
        ..TextGroup::INITIAL
    }
}

/// The string `text-emphasis-style` marks text with, where it marks it:
/// a string's first character, or a keyword's character.
pub(crate) fn emphasis_mark_string(computed: &ComputedValues) -> Option<String> {
    use style::values::specified::text::{TextEmphasisFillMode, TextEmphasisShapeKeyword};
    match computed.get_inherited_text().text_emphasis_style {
        StyloTextEmphasisStyle::None => None,
        StyloTextEmphasisStyle::String(ref string) => string.chars().next().map(String::from),
        StyloTextEmphasisStyle::Keyword { fill, shape } => {
            let (filled, open) = match shape {
                TextEmphasisShapeKeyword::Dot => ('\u{2022}', '\u{25E6}'),
                TextEmphasisShapeKeyword::Circle => ('\u{25CF}', '\u{25CB}'),
                TextEmphasisShapeKeyword::DoubleCircle => ('\u{25C9}', '\u{25CE}'),
                TextEmphasisShapeKeyword::Triangle => ('\u{25B2}', '\u{25B3}'),
                TextEmphasisShapeKeyword::Sesame => ('\u{FE45}', '\u{FE46}'),
            };
            Some(String::from(if fill == TextEmphasisFillMode::Open {
                open
            } else {
                filled
            }))
        }
    }
}

/// `text-emphasis` as far as layout reads it: whether marks are set, and on
/// which side. `auto` is `over right`, which is Chrome's initial value.
fn emphasis(computed: &ComputedValues) -> TextEmphasis {
    let marks = !matches!(
        computed.clone_text_emphasis_style(),
        StyloTextEmphasisStyle::None
    );
    let position = computed.clone_text_emphasis_position();
    TextEmphasis {
        marks,
        position: EmphasisPosition {
            side: if position.contains(StyloTextEmphasisPosition::UNDER) {
                EmphasisSide::Under
            } else {
                EmphasisSide::Over
            },
            vertical_side: if position.contains(StyloTextEmphasisPosition::LEFT) {
                EmphasisVerticalSide::Left
            } else {
                EmphasisVerticalSide::Right
            },
        },
        skip: EmphasisSkip::INITIAL,
    }
}

fn line_group(computed: &ComputedValues, scale: f32) -> LineGroup {
    let font = computed.get_font();
    let initial_letter = computed.clone_initial_letter();
    LineGroup {
        height: match font.line_height {
            StyloLineHeight::Normal => LineHeight::Normal,
            StyloLineHeight::Number(number) => LineHeight::Factor(number.0),
            StyloLineHeight::Length(length) => LineHeight::Px(length.0.px() * scale),
        },
        vertical_align: vertical_align(computed, scale),
        dominant_baseline: match computed.clone_dominant_baseline() {
            StyloDominantBaseline::Alphabetic => DominantBaseline::Alphabetic,
            StyloDominantBaseline::Ideographic => DominantBaseline::Ideographic,
            StyloDominantBaseline::Central | StyloDominantBaseline::Middle => {
                DominantBaseline::Central
            }
            StyloDominantBaseline::Mathematical => DominantBaseline::Mathematical,
            _ => DominantBaseline::Auto,
        },
        // A sink left out, or `drop`, is the size, floored; `raise` is 1.
        initial_letter: if initial_letter.size > 0.0 {
            InitialLetter {
                size: initial_letter.size,
                sink: match initial_letter.sink {
                    StyloInitialLetterSink::Integer(sink) if sink > 0 => sink as u32,
                    StyloInitialLetterSink::Raise => 1,
                    _ => (initial_letter.size.floor() as u32).max(1),
                },
                align: InitialLetterAlign::Alphabetic,
            }
        } else {
            InitialLetter::NONE
        },
        text_box_trim: text_box_trim(computed.clone_text_box_trim()),
        text_box_edge: text_box_edge(computed.clone_text_box_edge()),
        ..LineGroup::INITIAL
    }
}

fn text_box_trim(trim: StyloTextBoxTrim) -> TextBoxTrim {
    match (
        trim.contains(StyloTextBoxTrim::TRIM_START),
        trim.contains(StyloTextBoxTrim::TRIM_END),
    ) {
        (true, true) => TextBoxTrim::TrimBoth,
        (true, false) => TextBoxTrim::TrimStart,
        (false, true) => TextBoxTrim::TrimEnd,
        (false, false) => TextBoxTrim::None,
    }
}

fn text_box_edge(edge: StyloTextBoxEdge) -> TextBoxEdge {
    match edge {
        StyloTextBoxEdge::Auto => TextBoxEdge::AUTO,
        StyloTextBoxEdge::TextEdge(edge) => TextBoxEdge {
            over: match edge.over {
                TextEdgeKeyword::Cap => TextEdgeOver::Cap,
                TextEdgeKeyword::Ex => TextEdgeOver::Ex,
                TextEdgeKeyword::Ideographic => TextEdgeOver::Ideographic,
                TextEdgeKeyword::IdeographicInk => TextEdgeOver::IdeographicInk,
                _ => TextEdgeOver::Text,
            },
            under: match edge.under {
                TextEdgeKeyword::Alphabetic => TextEdgeUnder::Alphabetic,
                TextEdgeKeyword::Ideographic => TextEdgeUnder::Ideographic,
                TextEdgeKeyword::IdeographicInk => TextEdgeUnder::IdeographicInk,
                _ => TextEdgeUnder::Text,
            },
        },
    }
}

/// Margin, border and padding, and `box-decoration-break`.
///
/// A border width is snapped as CSS snaps it, in device pixels: one under a
/// pixel to a pixel, and any other down to a whole one.
fn edges_group(computed: &ComputedValues, scale: f32, basis: &Basis) -> EdgesGroup {
    let margin = computed.get_margin();
    let padding = computed.get_padding();
    let border = computed.get_border();
    let margin_side = |value: &GenericMargin<StyloLengthPercentage>| match value {
        GenericMargin::LengthPercentage(length) => of_basis(length, basis, scale),
        _ => 0.0,
    };
    let border_side = |width: &BorderSideWidth, style: BorderStyle| {
        let width = width.0.to_f32_px() * scale;
        if style.none_or_hidden() || width <= 0.0 {
            0.0
        } else {
            width.floor().max(1.0)
        }
    };
    EdgesGroup {
        margin: Sides {
            top: margin_side(&margin.margin_top),
            right: margin_side(&margin.margin_right),
            bottom: margin_side(&margin.margin_bottom),
            left: margin_side(&margin.margin_left),
        },
        border: Sides {
            top: border_side(&border.border_top_width, border.border_top_style),
            right: border_side(&border.border_right_width, border.border_right_style),
            bottom: border_side(&border.border_bottom_width, border.border_bottom_style),
            left: border_side(&border.border_left_width, border.border_left_style),
        },
        padding: Sides {
            top: of_basis(&padding.padding_top.0, basis, scale),
            right: of_basis(&padding.padding_right.0, basis, scale),
            bottom: of_basis(&padding.padding_bottom.0, basis, scale),
            left: of_basis(&padding.padding_left.0, basis, scale),
        },
        decoration_break: match computed.clone_box_decoration_break() {
            StyloBoxDecorationBreak::Slice => BoxDecorationBreak::Slice,
            StyloBoxDecorationBreak::Clone => BoxDecorationBreak::Clone,
        },
    }
}

/// `vertical-align`, which Stylo keeps as its two longhands.
fn vertical_align(computed: &ComputedValues, scale: f32) -> VerticalAlign {
    match computed.clone_baseline_shift() {
        GenericBaselineShift::Keyword(BaselineShiftKeyword::Sub) => return VerticalAlign::Sub,
        GenericBaselineShift::Keyword(BaselineShiftKeyword::Super) => return VerticalAlign::Super,
        GenericBaselineShift::Keyword(BaselineShiftKeyword::Top) => return VerticalAlign::Top,
        GenericBaselineShift::Keyword(BaselineShiftKeyword::Bottom) => {
            return VerticalAlign::Bottom;
        }
        GenericBaselineShift::Keyword(BaselineShiftKeyword::Center) => {
            return VerticalAlign::Middle;
        }
        GenericBaselineShift::Length(shift) => match shift.unpack() {
            Unpacked::Length(length) if length.px() != 0.0 => {
                return VerticalAlign::Px(length.px() * scale);
            }
            Unpacked::Percentage(percentage) if percentage.0 != 0.0 => {
                return VerticalAlign::Fraction(percentage.0);
            }
            _ => {}
        },
    }
    #[allow(unreachable_patterns)]
    match computed.clone_alignment_baseline() {
        AlignmentBaseline::TextTop => VerticalAlign::TextTop,
        AlignmentBaseline::TextBottom => VerticalAlign::TextBottom,
        AlignmentBaseline::Middle => VerticalAlign::Middle,
        _ => VerticalAlign::Baseline,
    }
}

/// Which way the block's lines run. The vendored Stylo computes the
/// sideways modes and `text-orientation`.
///
/// `is_sideways` is not the question here: it also answers yes to
/// `text-orientation: sideways`, which turns the glyphs of a vertical block
/// without changing which way its lines run. Only the writing mode's own
/// sideways values reverse the lines, and that is `VERTICAL_SIDEWAYS`.
pub(crate) fn writing_mode(computed: &ComputedValues) -> WritingMode {
    let mode = computed.writing_mode;
    if mode.intersects(StyloWritingMode::VERTICAL_SIDEWAYS) {
        if mode.is_vertical_lr() {
            WritingMode::SidewaysLr
        } else {
            WritingMode::SidewaysRl
        }
    } else if mode.is_vertical_lr() {
        WritingMode::VerticalLr
    } else if mode.is_vertical() {
        WritingMode::VerticalRl
    } else {
        WritingMode::HorizontalTb
    }
}

/// The block's styling: its own style and its `::first-line` style, and the
/// properties with one value per block container, read from `computed`.
///
/// `container` is the box whose overflow decides whether `text-overflow`
/// applies, which is the block itself or, for an anonymous block, the box
/// around it.
pub(crate) fn block_style<'a>(
    style: &'a ComputedStyle<'a>,
    first_line: Option<&'a ComputedStyle<'a>>,
    computed: &ComputedValues,
    container: &ComputedValues,
    scale: f32,
) -> ComputedBlockStyle<'a> {
    let text = computed.get_inherited_text();
    let text_align = match computed.clone_text_align() {
        TextAlignKeyword::Start => TextAlign::Start,
        TextAlignKeyword::End => TextAlign::End,
        TextAlignKeyword::Left | TextAlignKeyword::MozLeft => TextAlign::Left,
        TextAlignKeyword::Right | TextAlignKeyword::MozRight => TextAlign::Right,
        TextAlignKeyword::Center | TextAlignKeyword::MozCenter => TextAlign::Center,
        TextAlignKeyword::Justify => TextAlign::Justify,
    };
    let indent = computed.clone_text_indent();
    // `plaintext` on the block takes each paragraph's direction from its own
    // first strong character.
    let direction = if style.bidi.unicode_bidi == UnicodeBidi::Plaintext {
        BaseDirection::Auto
    } else if style.bidi.direction == Direction::Rtl {
        BaseDirection::Rtl
    } else {
        BaseDirection::Ltr
    };
    // An ellipsis where the block clips what overflows it, as Chrome's
    // `ShouldTruncateOverflowingText` asks. The end side's value, which is
    // the only side a line overflows at: Stylo keeps it in `second` either
    // way, a single value being the end with the start left at `clip`.
    let clips = !matches!(
        container.clone_overflow_x(),
        style::values::computed::Overflow::Visible
    );
    let text_overflow = match &computed.get_text().text_overflow.second {
        StyloTextOverflowSide::Ellipsis | StyloTextOverflowSide::String(_) if clips => {
            TextOverflow::Ellipsis
        }
        _ => TextOverflow::Clip,
    };
    ComputedBlockStyle {
        first_line,
        direction,
        writing_mode: writing_mode(computed),
        text_align,
        text_align_last: match text.text_align_last {
            StyloTextAlignLast::Auto => TextAlignLast::Auto,
            StyloTextAlignLast::Start => TextAlignLast::Start,
            StyloTextAlignLast::End => TextAlignLast::End,
            StyloTextAlignLast::Left => TextAlignLast::Left,
            StyloTextAlignLast::Right => TextAlignLast::Right,
            StyloTextAlignLast::Center => TextAlignLast::Center,
            StyloTextAlignLast::Justify => TextAlignLast::Justify,
        },
        text_indent: TextIndent {
            amount: length_percentage(&indent.length, scale),
            hanging: indent.hanging,
            each_line: indent.each_line,
        },
        text_wrap_style: match computed.clone_text_wrap_style() {
            StyloTextWrapStyle::Auto => TextWrapStyle::Auto,
            StyloTextWrapStyle::Stable => TextWrapStyle::Stable,
            StyloTextWrapStyle::Balance => TextWrapStyle::Balance,
            StyloTextWrapStyle::Pretty => TextWrapStyle::Pretty,
        },
        text_overflow,
        // `line-clamp`, or where it is `none`, `-webkit-line-clamp`, which
        // Chrome applies to a `-webkit-box`; the box is not required here,
        // Stylo's servo build having no `-webkit-box` to ask for.
        line_clamp: {
            let clamp = computed.clone_line_clamp();
            match clamp.max_lines.lines_value() {
                Some(lines) => LineClamp::Lines(lines.0.max(0) as u32),
                None if clamp.max_lines.is_auto() => LineClamp::Auto,
                None => LineClamp::None,
            }
        },
        text_box_trim: text_box_trim(computed.clone_text_box_trim()),
        text_box_edge: text_box_edge(computed.clone_text_box_edge()),
        ..ComputedBlockStyle::new(style)
    }
}

/// `ruby-position`.
///
/// `alternate` alone is Stylo's `AlternateOver`; the vendored Stylo's initial
/// value is Chrome's `over`, so the two are told apart.
pub(super) fn ruby_position(computed: &ComputedValues) -> RubyPosition {
    match computed.clone_ruby_position() {
        StyloRubyPosition::AlternateOver => RubyPosition::Alternate,
        StyloRubyPosition::AlternateUnder => RubyPosition::AlternateUnder,
        StyloRubyPosition::Over => RubyPosition::Over,
        StyloRubyPosition::Under => RubyPosition::Under,
    }
}

/// A generic family as fontwich names it.
fn generic_family(generic: StyloGenericFontFamily) -> GenericFamily {
    match generic {
        StyloGenericFontFamily::None | StyloGenericFontFamily::SansSerif => {
            GenericFamily::SansSerif
        }
        StyloGenericFontFamily::Serif => GenericFamily::Serif,
        StyloGenericFontFamily::Monospace => GenericFamily::Monospace,
        StyloGenericFontFamily::Cursive => GenericFamily::Cursive,
        StyloGenericFontFamily::Fantasy => GenericFamily::Fantasy,
        StyloGenericFontFamily::SystemUi => GenericFamily::SystemUi,
    }
}
