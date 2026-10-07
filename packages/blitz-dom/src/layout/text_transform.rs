//! Case and mathematical italic mapping for the CSS `text-transform` property.
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
use style::properties::ComputedValues;
use style::values::computed::TextTransform;
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

/// The maximum number of bytes of preceding text kept as context for finding word boundaries.
const MAX_CONTEXT_LEN: usize = 32;

/// The case or mathematical transform (and language) for an element's text content.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CaseTransform {
    kind: TextTransform,
    lang: LanguageIdentifier,
}

impl CaseTransform {
    pub(crate) const NONE: Self = Self {
        kind: TextTransform::NONE,
        lang: LanguageIdentifier::UNKNOWN,
    };

    pub(crate) fn from_style(style: &ComputedValues) -> Self {
        let kind = style.clone_text_transform() & TextTransform::CASE_TRANSFORMS;
        if kind.is_empty() {
            return Self::NONE;
        }
        if kind == TextTransform::MATH_AUTO {
            return if cfg!(feature = "text-transform-math") {
                Self {
                    kind,
                    lang: LanguageIdentifier::UNKNOWN,
                }
            } else {
                Self::NONE
            };
        }
        let lang = LanguageIdentifier::try_from_str(&style.get_font()._x_lang.0)
            .map(casing_language)
            .unwrap_or(LanguageIdentifier::UNKNOWN);
        Self { kind, lang }
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
        if transform.kind.is_empty() {
            return text;
        }

        if text.is_ascii() && !transform.has_turkic_casing() {
            match transform.kind {
                TextTransform::UPPERCASE => {
                    return map_ascii(
                        text,
                        &mut self.output,
                        u8::is_ascii_lowercase,
                        str::make_ascii_uppercase,
                    );
                }
                TextTransform::LOWERCASE => {
                    return map_ascii(
                        text,
                        &mut self.output,
                        u8::is_ascii_uppercase,
                        str::make_ascii_lowercase,
                    );
                }
                _ => {}
            }
        }

        let mut output = OutputSink::new(text, &mut self.output);
        match transform.kind {
            TextTransform::UPPERCASE => uppercase(text, &transform.lang, &mut output),
            TextTransform::LOWERCASE => lowercase(text, &transform.lang, &mut output),
            #[cfg(feature = "text-transform-math")]
            TextTransform::MATH_AUTO => math_auto(text, &mut output),
            TextTransform::CAPITALIZE => {
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
            _ => return text,
        }
        output.finish()
    }
}

/// Maps single-character text nodes to mathematical italic characters.
/// <https://w3c.github.io/mathml-core/#italic-mappings>
#[cfg(feature = "text-transform-math")]
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
}

impl<'a> OutputSink<'a> {
    fn new(original: &'a str, buffer: &'a mut String) -> Self {
        Self {
            original,
            unchanged_len: Some(0),
            buffer,
        }
    }

    fn push_str(&mut self, s: &str) {
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
            kind,
            lang: casing_language(LanguageIdentifier::try_from_str(lang).unwrap()),
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
            kind: TextTransform::CAPITALIZE,
            lang: LanguageIdentifier::UNKNOWN,
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
            kind: TextTransform::CAPITALIZE,
            lang: LanguageIdentifier::UNKNOWN,
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
                    kind,
                    lang: LanguageIdentifier::UNKNOWN,
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
    #[cfg(feature = "text-transform-math")]
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
    #[cfg(feature = "text-transform-math")]
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
            kind: TextTransform::MATH_AUTO,
            lang: LanguageIdentifier::UNKNOWN,
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

    #[test]
    #[cfg(not(feature = "text-transform-math"))]
    fn math_auto_disabled() {
        assert_eq!(
            transform(TextTransform::MATH_AUTO, "und", &["a", "h", "α", "∂"]),
            ["a", "h", "α", "∂"],
        );
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
