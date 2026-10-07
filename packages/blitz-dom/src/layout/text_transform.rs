//! Case, mathematical italic, full-width and full-size kana mapping for the CSS `text-transform`
//! property.
//!
//! <https://drafts.csswg.org/css-text/#text-transform-property>

#[cfg(feature = "text-transform-icu")]
use icu_casemap::options::{LeadingAdjustment, TitlecaseOptions, TrailingCase};
#[cfg(feature = "text-transform-icu")]
use icu_casemap::{CaseMapper, CaseMapperBorrowed, TitlecaseMapper, TitlecaseMapperBorrowed};
use icu_locale_core::LanguageIdentifier;
use icu_properties::props::{GeneralCategory, GeneralCategoryGroup};
use icu_properties::{CodePointMapData, CodePointMapDataBorrowed};
use icu_segmenter::{WordSegmenter, WordSegmenterBorrowed, options::WordBreakInvariantOptions};
use parley::{Brush, TreeBuilder};
use style::computed_values::white_space_collapse::T as WhiteSpaceCollapse;
use style::properties::ComputedValues;
use style::values::computed::TextTransform;
use style::values::specified::text::TextTransformCase;
#[cfg(feature = "text-transform-icu")]
use writeable::Writeable;

#[cfg(feature = "text-transform-icu")]
const CASE_MAPPER: CaseMapperBorrowed<'static> = CaseMapper::new();
#[cfg(feature = "text-transform-icu")]
const TITLECASE_MAPPER: TitlecaseMapperBorrowed<'static> = TitlecaseMapper::new();
const WORD_SEGMENTER: WordSegmenterBorrowed<'static> =
    WordSegmenter::new_for_non_complex_scripts(WordBreakInvariantOptions::default());
const GENERAL_CATEGORY: CodePointMapDataBorrowed<'static, GeneralCategory> =
    CodePointMapData::<GeneralCategory>::new();

const WIDTH_TRANSFORMS: TextTransform =
    TextTransform::FULL_WIDTH.union(TextTransform::FULL_SIZE_KANA);

/// The maximum number of bytes of preceding text kept as context for finding word boundaries.
const MAX_CONTEXT_LEN: usize = 32;

/// The text transforms (and language) for an element's text content.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CaseTransform {
    text_transform: TextTransform,
    /// Determines which spaces `full-width` maps (only used for `full-width`).
    white_space_collapse: WhiteSpaceCollapse,
    lang: LanguageIdentifier,
}

impl CaseTransform {
    pub(crate) const NONE: Self = Self {
        text_transform: TextTransform::NONE,
        white_space_collapse: WhiteSpaceCollapse::Collapse,
        lang: LanguageIdentifier::UNKNOWN,
    };

    pub(crate) fn from_style(style: &ComputedValues) -> Self {
        let text_transform = style.clone_text_transform();
        if text_transform.is_empty() {
            return Self::NONE;
        }
        let white_space_collapse = if text_transform.contains(TextTransform::FULL_WIDTH) {
            style.clone_white_space_collapse()
        } else {
            WhiteSpaceCollapse::Collapse
        };
        let lang = match text_transform.case() {
            TextTransformCase::None | TextTransformCase::MathAuto => LanguageIdentifier::UNKNOWN,
            _ => LanguageIdentifier::try_from_str(&style.get_font()._x_lang.0)
                .map(casing_language)
                .unwrap_or(LanguageIdentifier::UNKNOWN),
        };
        Self {
            text_transform,
            white_space_collapse,
            lang,
        }
    }

    /// Whether the language has case mappings for ASCII characters that differ from the
    /// default ones (`i` ↔ `İ` and `ı` ↔ `I`).
    fn has_turkic_casing(&self) -> bool {
        cfg!(feature = "text-transform-icu") && matches!(self.lang.language.as_str(), "tr" | "az")
    }
}

/// Language-specific case mapping rules don't apply if an explicit script subtag contradicts
/// the language's script (e.g. `tr-Cyrl`).
///
/// <https://drafts.csswg.org/css-text-3/#script-tagging>
fn casing_language(lang: LanguageIdentifier) -> LanguageIdentifier {
    let Some(script) = lang.script else {
        return lang;
    };
    // The languages with tailored case mappings in ICU4X.
    let language_script = match lang.language.as_str() {
        "az" | "lt" | "nl" | "tr" => "Latn",
        "el" => "Grek",
        "hy" => "Armn",
        _ => return lang,
    };
    if script.as_str() == language_script {
        lang
    } else {
        LanguageIdentifier::UNKNOWN
    }
}

/// Applies text transforms to the sequence of text nodes in an inline formatting context.
///
/// `capitalize` operates on words, which may span multiple text nodes, so the text already
/// pushed to the [`TreeBuilder`] is used as context for word segmentation. Only look-behind
/// is needed: whether a word boundary precedes a letter never depends on the text that
/// follows it.
#[derive(Default)]
pub(crate) struct TextTransformer {
    /// The offset in the builder's text of the last forced word boundary.
    context_start: usize,
    /// Reused buffer for the context followed by the text to be capitalized.
    segmentation_buffer: String,
    /// Reused buffer for the transformed text.
    output: String,
}

impl TextTransformer {
    /// Forces a word boundary (e.g. at an atomic inline or a forced line break).
    pub(crate) fn word_break<B: Brush>(&mut self, builder: &TreeBuilder<'_, B>) {
        self.context_start = builder.text().len();
    }

    /// Transforms the content of a text node that is about to be pushed to `builder`.
    pub(crate) fn transform<'a, B: Brush>(
        &'a mut self,
        text: &'a str,
        transform: &CaseTransform,
        builder: &TreeBuilder<'_, B>,
    ) -> &'a str {
        if transform.text_transform.is_empty() {
            return text;
        }

        let case = transform.text_transform.case();
        if text.is_ascii()
            && !transform.has_turkic_casing()
            && !transform.text_transform.contains(TextTransform::FULL_WIDTH)
        {
            match case {
                TextTransformCase::Uppercase => {
                    return map_ascii(
                        text,
                        &mut self.output,
                        u8::is_ascii_lowercase,
                        str::make_ascii_uppercase,
                    );
                }
                TextTransformCase::Lowercase => {
                    return map_ascii(
                        text,
                        &mut self.output,
                        u8::is_ascii_uppercase,
                        str::make_ascii_lowercase,
                    );
                }
                TextTransformCase::None => return text,
                _ => {}
            }
        }

        let mut output = OutputSink::new(text, &mut self.output);
        output.width = transform.text_transform & WIDTH_TRANSFORMS;
        output.white_space_collapse = transform.white_space_collapse;
        match case {
            TextTransformCase::None => output.push_str(text),
            TextTransformCase::Uppercase => uppercase(text, &transform.lang, &mut output),
            TextTransformCase::Lowercase => lowercase(text, &transform.lang, &mut output),
            TextTransformCase::MathAuto => math_auto(text, &mut output),
            TextTransformCase::Capitalize => {
                // Pending (collapsed) whitespace always ends the preceding word, so no
                // context is needed.
                let context = if builder.has_pending_whitespace() {
                    ""
                } else {
                    let preceding = &builder.text()[self.context_start..];
                    let start = ceil_char_boundary(
                        preceding,
                        preceding.len().saturating_sub(MAX_CONTEXT_LEN),
                    );
                    &preceding[start..]
                };
                capitalize(
                    context,
                    text,
                    &transform.lang,
                    &mut self.segmentation_buffer,
                    &mut output,
                );
            }
        }
        output.finish()
    }
}

/// Maps single-character text nodes to mathematical italic characters.
/// <https://w3c.github.io/mathml-core/#italic-mappings>
fn math_auto(text: &str, output: &mut OutputSink<'_>) {
    let mut chars = text.chars();
    let Some(c) = chars.next().filter(|_| chars.next().is_none()) else {
        output.push_str(text);
        return;
    };
    let codepoint = match c {
        'A'..='Z' => c as u32 + 0x1D3F3,
        'h' => 0x210E,
        'a'..='z' => c as u32 + 0x1D3ED,
        '\u{131}' => 0x1D6A4,
        '\u{237}' => 0x1D6A5,
        '\u{391}'..='\u{3A1}' | '\u{3A3}'..='\u{3A9}' => c as u32 + 0x1D351,
        '\u{3B1}'..='\u{3C9}' => c as u32 + 0x1D34B,
        '\u{3F4}' => 0x1D6F3,
        '\u{2207}' => 0x1D6FB,
        '\u{2202}' => 0x1D715,
        '\u{3F5}' => 0x1D716,
        '\u{3D1}' => 0x1D717,
        '\u{3F0}' => 0x1D718,
        '\u{3D5}' => 0x1D719,
        '\u{3F1}' => 0x1D71A,
        '\u{3D6}' => 0x1D71B,
        _ => {
            output.push_str(text);
            return;
        }
    };
    let mapped = char::from_u32(codepoint).unwrap();
    output.push_str(mapped.encode_utf8(&mut [0; 4]));
}

/// Whether Parley collapses `c` with the given `white-space-collapse`.
fn is_collapsible(white_space_collapse: WhiteSpaceCollapse, c: char) -> bool {
    match white_space_collapse {
        WhiteSpaceCollapse::Collapse => c.is_ascii_whitespace(),
        WhiteSpaceCollapse::PreserveBreaks => matches!(c, ' ' | '\t'),
        WhiteSpaceCollapse::Preserve | WhiteSpaceCollapse::BreakSpaces => false,
    }
}

/// Maps `c` to its full-width form: the reverse of its `<wide>` decomposition mapping or its
/// `<narrow>` decomposition mapping, if any.
/// <https://drafts.csswg.org/css-text-3/#text-transform-mapping>
pub fn full_width(c: char) -> char {
    const HALFWIDTH_KATAKANA: [char; 63] = [
        '\u{3002}', '\u{300C}', '\u{300D}', '\u{3001}', '\u{30FB}', '\u{30F2}', '\u{30A1}',
        '\u{30A3}', '\u{30A5}', '\u{30A7}', '\u{30A9}', '\u{30E3}', '\u{30E5}', '\u{30E7}',
        '\u{30C3}', '\u{30FC}', '\u{30A2}', '\u{30A4}', '\u{30A6}', '\u{30A8}', '\u{30AA}',
        '\u{30AB}', '\u{30AD}', '\u{30AF}', '\u{30B1}', '\u{30B3}', '\u{30B5}', '\u{30B7}',
        '\u{30B9}', '\u{30BB}', '\u{30BD}', '\u{30BF}', '\u{30C1}', '\u{30C4}', '\u{30C6}',
        '\u{30C8}', '\u{30CA}', '\u{30CB}', '\u{30CC}', '\u{30CD}', '\u{30CE}', '\u{30CF}',
        '\u{30D2}', '\u{30D5}', '\u{30D8}', '\u{30DB}', '\u{30DE}', '\u{30DF}', '\u{30E0}',
        '\u{30E1}', '\u{30E2}', '\u{30E4}', '\u{30E6}', '\u{30E8}', '\u{30E9}', '\u{30EA}',
        '\u{30EB}', '\u{30EC}', '\u{30ED}', '\u{30EF}', '\u{30F3}', '\u{3099}', '\u{309A}',
    ];
    let codepoint = match c {
        ' ' => 0x3000,
        '!'..='~' => c as u32 + 0xFEE0,
        '\u{A2}' => 0xFFE0,
        '\u{A3}' => 0xFFE1,
        '\u{A5}' => 0xFFE5,
        '\u{A6}' => 0xFFE4,
        '\u{AC}' => 0xFFE2,
        '\u{AF}' => 0xFFE3,
        '\u{20A9}' => 0xFFE6,
        '\u{2985}' => 0xFF5F,
        '\u{2986}' => 0xFF60,
        '\u{FF61}'..='\u{FF9F}' => return HALFWIDTH_KATAKANA[c as usize - 0xFF61],
        // Halfwidth Hangul
        '\u{FFA0}' => 0x3164,
        '\u{FFA1}'..='\u{FFBE}' => c as u32 - 0xCE70,
        '\u{FFC2}'..='\u{FFC7}' => c as u32 - 0xCE73,
        '\u{FFCA}'..='\u{FFCF}' => c as u32 - 0xCE75,
        '\u{FFD2}'..='\u{FFD7}' => c as u32 - 0xCE77,
        '\u{FFDA}'..='\u{FFDC}' => c as u32 - 0xCE79,
        '\u{FFE8}' => 0x2502,
        '\u{FFE9}'..='\u{FFEC}' => c as u32 - 0xDE59,
        '\u{FFED}' => 0x25A0,
        '\u{FFEE}' => 0x25CB,
        _ => return c,
    };
    char::from_u32(codepoint).unwrap()
}

/// Maps small kana to full-size kana.
/// <https://drafts.csswg.org/css-text-3/#small-kana>
pub fn full_size_kana(c: char) -> char {
    const SMALL_KATAKANA_EXTENSIONS: [char; 16] = [
        '\u{30AF}', '\u{30B7}', '\u{30B9}', '\u{30C8}', '\u{30CC}', '\u{30CF}', '\u{30D2}',
        '\u{30D5}', '\u{30D8}', '\u{30DB}', '\u{30E0}', '\u{30E9}', '\u{30EA}', '\u{30EB}',
        '\u{30EC}', '\u{30ED}',
    ];
    let codepoint = match c {
        '\u{3041}' | '\u{3043}' | '\u{3045}' | '\u{3047}' | '\u{3049}' | '\u{3063}'
        | '\u{3083}' | '\u{3085}' | '\u{3087}' | '\u{308E}' | '\u{30A1}' | '\u{30A3}'
        | '\u{30A5}' | '\u{30A7}' | '\u{30A9}' | '\u{30C3}' | '\u{30E3}' | '\u{30E5}'
        | '\u{30E7}' | '\u{30EE}' => c as u32 + 1,
        '\u{3095}' => 0x304B,
        '\u{3096}' => 0x3051,
        '\u{30F5}' => 0x30AB,
        '\u{30F6}' => 0x30B1,
        '\u{31F0}'..='\u{31FF}' => return SMALL_KATAKANA_EXTENSIONS[c as usize - 0x31F0],
        '\u{FF67}'..='\u{FF6B}' => c as u32 + 10,
        '\u{FF6C}' => 0xFF94,
        '\u{FF6D}' => 0xFF95,
        '\u{FF6E}' => 0xFF96,
        '\u{FF6F}' => 0xFF82,
        '\u{1B132}' => 0x3053,
        '\u{1B150}'..='\u{1B152}' => c as u32 - 0x180C0,
        '\u{1B155}' => 0x30B3,
        '\u{1B164}'..='\u{1B167}' => c as u32 - 0x18074,
        _ => return c,
    };
    char::from_u32(codepoint).unwrap()
}

/// Case maps ASCII `text`, borrowing it if no bytes need mapping and otherwise copying it into
/// `buffer`.
fn map_ascii<'a>(
    text: &'a str,
    buffer: &'a mut String,
    needs_mapping: impl Fn(&u8) -> bool,
    map: impl FnOnce(&mut str),
) -> &'a str {
    let Some(first) = text.bytes().position(|b| needs_mapping(&b)) else {
        return text;
    };
    buffer.clear();
    buffer.push_str(text);
    map(&mut buffer[first..]);
    buffer
}

/// Collects transformed text, borrowing the original text if it is unchanged and otherwise
/// copying it into a reused buffer from the first difference onwards.
struct OutputSink<'a> {
    original: &'a str,
    /// The length of the output so far if it is a prefix of `original`, or `None` if the output
    /// has diverged and is in `buffer`.
    unchanged_len: Option<usize>,
    buffer: &'a mut String,
    /// `full-width` and/or `full-size-kana`, applied to each pushed character.
    width: TextTransform,
    white_space_collapse: WhiteSpaceCollapse,
    /// Whether the last character was content that a following collapsed space would separate
    /// from the next content (i.e. not whitespace or a segment break).
    after_content: bool,
    /// The output length at the start of the current sequence of collapsible whitespace.
    whitespace_start: Option<usize>,
}

impl<'a> OutputSink<'a> {
    fn new(original: &'a str, buffer: &'a mut String) -> Self {
        Self {
            original,
            unchanged_len: Some(0),
            buffer,
            width: TextTransform::NONE,
            white_space_collapse: WhiteSpaceCollapse::Collapse,
            after_content: false,
            whitespace_start: None,
        }
    }

    fn push_str(&mut self, s: &str) {
        if self.width.is_empty() {
            self.push_unmapped(s);
        } else {
            s.chars().for_each(|c| self.push_char(c));
        }
    }

    /// Pushes `c` after applying the `full-width` and `full-size-kana` mappings.
    ///
    /// Text transforms apply after white space collapsing, so `full-width` only maps preserved
    /// spaces, and a sequence of collapsible whitespace to U+3000 if it is between two pieces of
    /// content in this text node, where Parley would collapse it to a single space. Sequences at
    /// the start or end of the text node are left for Parley to collapse.
    fn push_char(&mut self, mut c: char) {
        if self.width.contains(TextTransform::FULL_WIDTH) {
            if is_collapsible(self.white_space_collapse, c) {
                self.whitespace_start.get_or_insert(self.len());
                self.push_unmapped(c.encode_utf8(&mut [0; 4]));
                return;
            }
            let is_segment_break = matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}');
            if let Some(whitespace_start) = self.whitespace_start.take()
                && self.after_content
                && !is_segment_break
            {
                self.truncate(whitespace_start);
                self.push_unmapped("\u{3000}");
            }
            self.after_content = !is_segment_break;
            c = full_width(c);
        }
        if self.width.contains(TextTransform::FULL_SIZE_KANA) {
            c = full_size_kana(c);
        }
        self.push_unmapped(c.encode_utf8(&mut [0; 4]));
    }

    fn len(&self) -> usize {
        self.unchanged_len.unwrap_or(self.buffer.len())
    }

    fn truncate(&mut self, len: usize) {
        match &mut self.unchanged_len {
            Some(unchanged_len) => *unchanged_len = len,
            None => self.buffer.truncate(len),
        }
    }

    fn push_unmapped(&mut self, s: &str) {
        if let Some(len) = self.unchanged_len {
            if self.original[len..].starts_with(s) {
                self.unchanged_len = Some(len + s.len());
                return;
            }
            self.buffer.clear();
            self.buffer.push_str(&self.original[..len]);
            self.unchanged_len = None;
        }
        self.buffer.push_str(s);
    }

    #[cfg(feature = "text-transform-icu")]
    fn write(&mut self, writeable: impl Writeable) {
        // Writing to an `OutputSink` is infallible.
        let _ = writeable.write_to(self);
    }

    fn finish(self) -> &'a str {
        match self.unchanged_len {
            Some(len) => &self.original[..len],
            None => self.buffer,
        }
    }
}

#[cfg(feature = "text-transform-icu")]
impl std::fmt::Write for OutputSink<'_> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.push_str(s);
        Ok(())
    }
}

/// Writes `text` to `output`, titlecasing the first typographic letter unit of each word and
/// leaving other characters unchanged. `context` is the text preceding `text` in the same word
/// segmentation context. `buffer` is scratch space.
fn capitalize(
    context: &str,
    text: &str,
    lang: &LanguageIdentifier,
    buffer: &mut String,
    output: &mut OutputSink<'_>,
) {
    let context_len = context.len();
    let combined = if context.is_empty() {
        text
    } else {
        buffer.clear();
        buffer.push_str(context);
        buffer.push_str(text);
        buffer
    };

    let mut segment_start = 0;
    for segment_end in WORD_SEGMENTER.segment_str(combined).skip(1) {
        if segment_end > context_len {
            let part_start = segment_start.max(context_len);
            let part = &combined[part_start..segment_end];

            // A word that started in a previous text node has already had its first
            // letter unit (if any) transformed (or not) as part of that text node.
            let first_letter_already_seen = combined[segment_start..part_start]
                .chars()
                .any(is_typographic_letter_unit);

            match part.find(is_typographic_letter_unit) {
                Some(letter_start) if !first_letter_already_seen => {
                    output.push_str(&part[..letter_start]);
                    titlecase_segment(&part[letter_start..], lang, output);
                }
                _ => output.push_str(part),
            }
        }
        segment_start = segment_end;
    }
}

#[cfg(feature = "text-transform-icu")]
fn uppercase(text: &str, lang: &LanguageIdentifier, output: &mut OutputSink<'_>) {
    output.write(CASE_MAPPER.uppercase(text, lang));
}

#[cfg(feature = "text-transform-icu")]
fn lowercase(text: &str, lang: &LanguageIdentifier, output: &mut OutputSink<'_>) {
    output.write(CASE_MAPPER.lowercase(text, lang));
}

/// Titlecases the first character of `text`, which must start with a typographic letter unit,
/// leaving the rest unchanged.
#[cfg(feature = "text-transform-icu")]
fn titlecase_segment(text: &str, lang: &LanguageIdentifier, output: &mut OutputSink<'_>) {
    let mut options = TitlecaseOptions::default();
    options.leading_adjustment = Some(LeadingAdjustment::None);
    options.trailing_case = Some(TrailingCase::Unchanged);
    output.write(TITLECASE_MAPPER.titlecase_segment(text, lang, options));
}

#[cfg(not(feature = "text-transform-icu"))]
fn uppercase(text: &str, _lang: &LanguageIdentifier, output: &mut OutputSink<'_>) {
    for c in text.chars() {
        push_chars(output, c.to_uppercase());
    }
}

#[cfg(not(feature = "text-transform-icu"))]
fn lowercase(text: &str, _lang: &LanguageIdentifier, output: &mut OutputSink<'_>) {
    // Only `str::to_lowercase` implements the context-sensitive final sigma rule.
    if text.contains('Σ') {
        output.push_str(&text.to_lowercase());
        return;
    }
    for c in text.chars() {
        push_chars(output, c.to_lowercase());
    }
}

/// Approximates titlecasing with uppercasing, as the standard library has no titlecase mapping.
#[cfg(not(feature = "text-transform-icu"))]
fn titlecase_segment(text: &str, _lang: &LanguageIdentifier, output: &mut OutputSink<'_>) {
    let mut chars = text.chars();
    if let Some(first) = chars.next() {
        push_chars(output, first.to_uppercase());
    }
    output.push_str(chars.as_str());
}

#[cfg(not(feature = "text-transform-icu"))]
fn push_chars(output: &mut OutputSink<'_>, chars: impl Iterator<Item = char>) {
    for c in chars {
        output.push_str(c.encode_utf8(&mut [0; 4]));
    }
}

/// <https://drafts.csswg.org/css-text/#typographic-letter-unit>
fn is_typographic_letter_unit(c: char) -> bool {
    let category = GENERAL_CATEGORY.get(c);
    GeneralCategoryGroup::Letter.contains(category)
        || GeneralCategoryGroup::Number.contains(category)
}

fn ceil_char_boundary(s: &str, mut index: usize) -> usize {
    while !s.is_char_boundary(index) {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use parley::{FontContext, LayoutContext, TextStyle};

    fn transform(kind: TextTransform, lang: &str, texts: &[&str]) -> Vec<String> {
        let transform = CaseTransform {
            text_transform: kind,
            lang: casing_language(LanguageIdentifier::try_from_str(lang).unwrap()),
            ..CaseTransform::NONE
        };
        with_builder(|builder, transformer| {
            texts
                .iter()
                .map(|text| push(builder, transformer, text, &transform))
                .collect()
        })
    }

    fn with_builder<R>(f: impl FnOnce(&mut TreeBuilder<'_, ()>, &mut TextTransformer) -> R) -> R {
        let mut font_ctx = FontContext::new();
        let mut layout_ctx = LayoutContext::new();
        let mut builder = layout_ctx.tree_builder(&mut font_ctx, 1.0, true, &TextStyle::default());
        f(&mut builder, &mut TextTransformer::default())
    }

    fn push(
        builder: &mut TreeBuilder<'_, ()>,
        transformer: &mut TextTransformer,
        text: &str,
        transform: &CaseTransform,
    ) -> String {
        let output = transformer.transform(text, transform, builder).to_owned();
        builder.push_text(&output);
        output
    }

    fn capitalize(texts: &[&str]) -> Vec<String> {
        transform(TextTransform::CAPITALIZE, "und", texts)
    }

    #[test]
    fn capitalize_words() {
        assert_eq!(capitalize(&["hello world"]), ["Hello World"]);
        assert_eq!(capitalize(&["foo-bar"]), ["Foo-Bar"]);
        assert_eq!(capitalize(&["john's apple"]), ["John's Apple"]);
        assert_eq!(capitalize(&["foo_bar"]), ["Foo_bar"]);
        assert_eq!(capitalize(&["cancel·lar"]), ["Cancel·lar"]);
        assert_eq!(capitalize(&["un·e député·e"]), ["Un·e Député·e"]);
        assert_eq!(capitalize(&["3rd place"]), ["3rd Place"]);
        assert_eq!(capitalize(&["aBC"]), ["ABC"]);
        // Circled letters are symbols, not letters
        assert_eq!(capitalize(&["ⓐⓐⓐ ⓑⓑⓑ"]), ["ⓐⓐⓐ ⓑⓑⓑ"]);
    }

    #[test]
    fn capitalize_skips_leading_punctuation() {
        assert_eq!(
            capitalize(&["({[-–\"«'.<?!transform"]),
            ["({[-–\"«'.<?!Transform"]
        );
    }

    #[test]
    #[cfg(feature = "text-transform-icu")]
    fn capitalize_uses_titlecase() {
        assert_eq!(capitalize(&["ǆǆ"]), ["ǅǆ"]);
        assert_eq!(capitalize(&["ßa"]), ["Ssa"]);
        assert_eq!(capitalize(&["ᾳᾳ"]), ["ᾼᾳ"]);
    }

    #[test]
    fn capitalize_across_text_nodes() {
        assert_eq!(
            capitalize(&["hel", "lo ", "world"]),
            ["Hel", "lo ", "World"]
        );
        assert_eq!(capitalize(&["don", "'t"]), ["Don", "'t"]);
        assert_eq!(capitalize(&["(", "abc"]), ["(", "Abc"]);
        assert_eq!(
            capitalize(&["hello ", " ", "world"]),
            ["Hello ", " ", "World"]
        );
    }

    #[test]
    fn capitalize_mid_word_text_node() {
        let capitalize = CaseTransform {
            text_transform: TextTransform::CAPITALIZE,
            lang: LanguageIdentifier::UNKNOWN,
            ..CaseTransform::NONE
        };
        with_builder(|builder, transformer| {
            assert_eq!(push(builder, transformer, "a", &CaseTransform::NONE), "a");
            assert_eq!(push(builder, transformer, "b", &capitalize), "b");
            assert_eq!(push(builder, transformer, "c", &CaseTransform::NONE), "c");
        });
    }

    #[test]
    fn capitalize_after_word_break() {
        let capitalize = CaseTransform {
            text_transform: TextTransform::CAPITALIZE,
            lang: LanguageIdentifier::UNKNOWN,
            ..CaseTransform::NONE
        };
        with_builder(|builder, transformer| {
            assert_eq!(push(builder, transformer, "abc", &capitalize), "Abc");
            transformer.word_break(builder);
            assert_eq!(push(builder, transformer, "def", &capitalize), "Def");
        });
    }

    #[test]
    fn capitalize_after_collapsed_whitespace() {
        assert_eq!(capitalize(&["hello ", "world"]), ["Hello ", "World"]);
        assert_eq!(capitalize(&["hello\n", "world"]), ["Hello\n", "World"]);
        assert_eq!(capitalize(&["hello ", "(world"]), ["Hello ", "(World"]);
        assert_eq!(
            capitalize(&["hello ", "\u{301}world"]),
            ["Hello ", "\u{301}World"]
        );
    }

    #[test]
    fn capitalize_long_words() {
        let long_word = "a".repeat(100);
        let output = capitalize(&[&long_word, "bc de"]);
        assert_eq!(output[1], "bc De");
    }

    #[test]
    #[cfg(feature = "text-transform-icu")]
    fn capitalize_language_sensitive() {
        assert_eq!(
            transform(TextTransform::CAPITALIZE, "nl", &["ijsland"]),
            ["IJsland"]
        );
    }

    #[test]
    fn unchanged_text_is_borrowed() {
        let cases = [
            (TextTransform::UPPERCASE, "ABC 123", true),
            (TextTransform::UPPERCASE, "ABc", false),
            (TextTransform::LOWERCASE, "abc 123", true),
            (TextTransform::LOWERCASE, "abC", false),
            (TextTransform::CAPITALIZE, "Hello World", true),
            (TextTransform::CAPITALIZE, "Hello world", false),
        ];
        with_builder(|builder, transformer| {
            for (kind, text, borrowed) in cases {
                let transform = CaseTransform {
                    text_transform: kind,
                    lang: LanguageIdentifier::UNKNOWN,
                    ..CaseTransform::NONE
                };
                transformer.word_break(builder);
                let output = transformer.transform(text, &transform, builder);
                assert_eq!(output.as_ptr() == text.as_ptr(), borrowed, "{text:?}");
                builder.push_text(" ");
            }
        });
    }

    #[test]
    #[cfg(feature = "text-transform-icu")]
    fn ascii_fast_path_matches_icu() {
        let ascii: String = (0..128u8).map(char::from).collect();
        for lang in ["und", "en", "lt", "nl", "el", "tr", "az"] {
            let locale = LanguageIdentifier::try_from_str(lang).unwrap();
            assert_eq!(
                transform(TextTransform::UPPERCASE, lang, &[&ascii]),
                [CASE_MAPPER.uppercase_to_string(&ascii, &locale)],
                "{lang}"
            );
            assert_eq!(
                transform(TextTransform::LOWERCASE, lang, &[&ascii]),
                [CASE_MAPPER.lowercase_to_string(&ascii, &locale)],
                "{lang}"
            );
        }
    }

    #[test]
    fn uppercase_and_lowercase() {
        assert_eq!(
            transform(TextTransform::UPPERCASE, "und", &["straße"]),
            ["STRASSE"]
        );
        assert_eq!(
            transform(TextTransform::LOWERCASE, "und", &["ABC"]),
            ["abc"]
        );
        assert_eq!(
            transform(TextTransform::LOWERCASE, "und", &["ΟΔΟΣ Σ"]),
            ["οδος σ"]
        );
    }

    #[test]
    fn math_auto_italic_mappings() {
        // MathML Core, Appendix C.1.
        let mappings = [
            ("ABCDEFGHIJKLMNOPQRSTUVWXYZ", "𝐴𝐵𝐶𝐷𝐸𝐹𝐺𝐻𝐼𝐽𝐾𝐿𝑀𝑁𝑂𝑃𝑄𝑅𝑆𝑇𝑈𝑉𝑊𝑋𝑌𝑍"),
            ("abcdefghijklmnopqrstuvwxyz", "𝑎𝑏𝑐𝑑𝑒𝑓𝑔ℎ𝑖𝑗𝑘𝑙𝑚𝑛𝑜𝑝𝑞𝑟𝑠𝑡𝑢𝑣𝑤𝑥𝑦𝑧"),
            ("ΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟΠΡϴΣΤΥΦΧΨΩ", "𝛢𝛣𝛤𝛥𝛦𝛧𝛨𝛩𝛪𝛫𝛬𝛭𝛮𝛯𝛰𝛱𝛲𝛳𝛴𝛵𝛶𝛷𝛸𝛹𝛺"),
            ("αβγδεζηθικλμνξοπρςστυφχψω", "𝛼𝛽𝛾𝛿𝜀𝜁𝜂𝜃𝜄𝜅𝜆𝜇𝜈𝜉𝜊𝜋𝜌𝜍𝜎𝜏𝜐𝜑𝜒𝜓𝜔"),
            ("ıȷ∇∂ϵϑϰϕϱϖ", "𝚤𝚥𝛻𝜕𝜖𝜗𝜘𝜙𝜚𝜛"),
        ];
        for (original, italic) in mappings {
            assert_eq!(original.chars().count(), italic.chars().count());
            for (input, expected) in original.chars().zip(italic.chars()) {
                assert_eq!(
                    transform(
                        TextTransform::MATH_AUTO,
                        "und",
                        &[input.encode_utf8(&mut [0; 4])]
                    ),
                    [expected.to_string()],
                    "U+{:04X}",
                    input as u32,
                );
            }
        }
    }

    #[test]
    fn math_auto_operates_on_each_text_node() {
        assert_eq!(
            transform(
                TextTransform::MATH_AUTO,
                "und",
                &["a", "b", "ab", "∂", "∂∇"]
            ),
            ["𝑎", "𝑏", "ab", "𝜕", "∂∇"],
        );
        assert_eq!(transform(TextTransform::MATH_AUTO, "tr", &["i"]), ["𝑖"]);
    }

    #[test]
    fn math_auto_unchanged_text_is_borrowed() {
        let transform = CaseTransform {
            text_transform: TextTransform::MATH_AUTO,
            lang: LanguageIdentifier::UNKNOWN,
            ..CaseTransform::NONE
        };
        with_builder(|builder, transformer| {
            for text in [
                "", "ab", "a ", " a", "a\u{301}", "∂∇", "1", "+", "é", "\u{3A2}", "ℎ", "𝑎", "😀",
            ] {
                let output = transformer.transform(text, &transform, builder);
                assert_eq!(output, text);
                assert_eq!(output.as_ptr(), text.as_ptr());
            }
            assert_eq!(transformer.output.capacity(), 0);
        });
    }

    fn transform_with(
        text_transform: TextTransform,
        white_space_collapse: WhiteSpaceCollapse,
        texts: &[&str],
    ) -> Vec<String> {
        let transform = CaseTransform {
            text_transform,
            white_space_collapse,
            ..CaseTransform::NONE
        };
        with_builder(|builder, transformer| {
            texts
                .iter()
                .map(|text| push(builder, transformer, text, &transform))
                .collect()
        })
    }

    fn full_width_with(white_space_collapse: WhiteSpaceCollapse, text: &str) -> String {
        transform_with(TextTransform::FULL_WIDTH, white_space_collapse, &[text]).remove(0)
    }

    #[test]
    fn full_width_mappings() {
        let ascii: String = ('!'..='~').collect();
        let wide: String = ('\u{FF01}'..='\u{FF5E}').collect();
        assert_eq!(full_width_with(WhiteSpaceCollapse::Collapse, &ascii), wide);
        // <wide> decompositions, reversed.
        assert_eq!(
            full_width_with(WhiteSpaceCollapse::Collapse, "¢£¬¯¦¥₩⦅⦆"),
            "￠￡￢￣￤￥￦｟｠"
        );
        // <narrow> decompositions.
        assert_eq!(
            full_width_with(WhiteSpaceCollapse::Collapse, "｡｢｣､･ｦｧｯｰｱﾝﾞﾟ"),
            "。「」、・ヲァッーアン\u{3099}\u{309A}"
        );
        assert_eq!(
            full_width_with(WhiteSpaceCollapse::Collapse, "\u{FFA0}ﾡﾾￂￇￊￏￒￗￚￜ"),
            "\u{3164}ㄱㅎㅏㅔㅕㅚㅛㅠㅡㅣ"
        );
        assert_eq!(
            full_width_with(WhiteSpaceCollapse::Collapse, "￨￩￪￫￬￭￮"),
            "│←↑→↓■○"
        );
        // Unassigned code points between the halfwidth Hangul blocks are left as is.
        assert_eq!(
            full_width_with(WhiteSpaceCollapse::Collapse, "\u{FFBF}\u{FFC8}\u{FFDD}"),
            "\u{FFBF}\u{FFC8}\u{FFDD}"
        );
        assert_eq!(
            full_width_with(WhiteSpaceCollapse::Collapse, "é\u{A0}あＡ"),
            "é\u{A0}あＡ"
        );
    }

    #[test]
    fn full_width_preserved_spaces() {
        for collapse in [
            WhiteSpaceCollapse::Preserve,
            WhiteSpaceCollapse::BreakSpaces,
        ] {
            assert_eq!(
                full_width_with(collapse, " a  b\t\n"),
                "\u{3000}ａ\u{3000}\u{3000}ｂ\t\n"
            );
        }
    }

    #[test]
    fn full_width_collapsed_spaces() {
        use WhiteSpaceCollapse::{Collapse, PreserveBreaks};
        assert_eq!(full_width_with(Collapse, "a b"), "ａ\u{3000}ｂ");
        assert_eq!(full_width_with(Collapse, "a \n\t b"), "ａ\u{3000}ｂ");
        // Whitespace at the start or end of the text node is collapsed by Parley.
        assert_eq!(full_width_with(Collapse, "  a  "), "  ａ  ");
        assert_eq!(full_width_with(Collapse, "   "), "   ");
        // Whitespace around a segment break is removed by Parley.
        assert_eq!(full_width_with(Collapse, "a \u{2028} b"), "ａ \u{2028} ｂ");
        assert_eq!(full_width_with(PreserveBreaks, "a \n b"), "ａ \n ｂ");
        assert_eq!(
            full_width_with(PreserveBreaks, "a  b\nc"),
            "ａ\u{3000}ｂ\nｃ"
        );
    }

    #[test]
    fn full_size_kana_mappings() {
        let small = "ぁぃぅぇぉゕゖ𛄲っゃゅょゎ𛅐𛅑𛅒ァィゥェォヵㇰヶ𛅕ㇱㇲッㇳㇴㇵㇶㇷㇸㇹㇺャュョㇻㇼㇽㇾㇿヮ𛅤𛅥𛅦𛅧ｧｨｩｪｫｬｭｮｯ";
        let full = "あいうえおかけこつやゆよわゐゑをアイウエオカクケコシスツトヌハヒフヘホムヤユヨラリルレロワヰヱヲンｱｲｳｴｵﾔﾕﾖﾂ";
        assert_eq!(small.chars().count(), full.chars().count());
        let output = transform_with(
            TextTransform::FULL_SIZE_KANA,
            WhiteSpaceCollapse::Collapse,
            &[small],
        );
        assert_eq!(output, [full]);
    }

    #[test]
    fn combined_transforms() {
        let collapse = WhiteSpaceCollapse::Collapse;
        let transform =
            |text_transform, text| transform_with(text_transform, collapse, &[text]).remove(0);
        assert_eq!(
            transform(
                TextTransform::UPPERCASE | TextTransform::FULL_WIDTH,
                "HELLO Transformed world"
            ),
            "ＨＥＬＬＯ　ＴＲＡＮＳＦＯＲＭＥＤ　ＷＯＲＬＤ"
        );
        assert_eq!(
            transform(
                TextTransform::CAPITALIZE | TextTransform::FULL_WIDTH,
                "HELLO Transformed world"
            ),
            "ＨＥＬＬＯ　Ｔｒａｎｓｆｏｒｍｅｄ　Ｗｏｒｌｄ"
        );
        assert_eq!(
            transform(
                TextTransform::UPPERCASE | TextTransform::FULL_WIDTH,
                "straße"
            ),
            "ＳＴＲＡＳＳＥ"
        );
        assert_eq!(
            transform(
                TextTransform::UPPERCASE | TextTransform::FULL_SIZE_KANA,
                "Katakana: ァィゥ"
            ),
            "KATAKANA: アイウ"
        );
        assert_eq!(
            transform(
                TextTransform::LOWERCASE
                    | TextTransform::FULL_WIDTH
                    | TextTransform::FULL_SIZE_KANA,
                "Hiragana: ぁぃ"
            ),
            "ｈｉｒａｇａｎａ：　あい"
        );
        // full-width is applied before full-size-kana.
        assert_eq!(
            transform(
                TextTransform::FULL_WIDTH | TextTransform::FULL_SIZE_KANA,
                "ｧｯ"
            ),
            "アツ"
        );
    }

    #[test]
    fn width_transforms_unchanged_text_is_borrowed() {
        let cases = [
            (TextTransform::FULL_SIZE_KANA, "abc あア ｱ"),
            (TextTransform::FULL_WIDTH, "ＡＢ\u{3000}あ"),
            (TextTransform::FULL_WIDTH, "  ＡＢ  "),
            (
                TextTransform::UPPERCASE | TextTransform::FULL_SIZE_KANA,
                "ABC",
            ),
            (TextTransform::UPPERCASE | TextTransform::FULL_WIDTH, "ＡＢ"),
        ];
        with_builder(|builder, transformer| {
            for (text_transform, text) in cases {
                let transform = CaseTransform {
                    text_transform,
                    ..CaseTransform::NONE
                };
                let output = transformer.transform(text, &transform, builder);
                assert_eq!(output, text);
                assert_eq!(output.as_ptr(), text.as_ptr(), "{text:?}");
            }
            assert_eq!(transformer.output.capacity(), 0);
        });
    }

    #[test]
    #[cfg(feature = "text-transform-icu")]
    fn uppercase_and_lowercase_language_sensitive() {
        assert_eq!(transform(TextTransform::UPPERCASE, "tr", &["i"]), ["İ"]);
        assert_eq!(transform(TextTransform::LOWERCASE, "tr", &["I"]), ["ı"]);
        assert_eq!(
            transform(TextTransform::LOWERCASE, "tr-Latn", &["I"]),
            ["ı"]
        );
        assert_eq!(
            transform(TextTransform::LOWERCASE, "tr-Cyrl", &["I"]),
            ["i"]
        );
    }
}
