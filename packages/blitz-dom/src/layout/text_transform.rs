//! Case mapping for the CSS `text-transform` property.
//!
//! <https://drafts.csswg.org/css-text/#text-transform-property>

use std::borrow::Cow;

use icu_casemap::options::{LeadingAdjustment, TitlecaseOptions, TrailingCase};
use icu_casemap::{CaseMapper, CaseMapperBorrowed, TitlecaseMapper, TitlecaseMapperBorrowed};
use icu_locale_core::LanguageIdentifier;
use icu_properties::props::{GeneralCategory, GeneralCategoryGroup};
use icu_properties::{CodePointMapData, CodePointMapDataBorrowed};
use icu_segmenter::{WordSegmenter, WordSegmenterBorrowed, options::WordBreakInvariantOptions};
use parley::{Brush, TreeBuilder};
use style::properties::ComputedValues;
use style::values::computed::TextTransform;

const CASE_MAPPER: CaseMapperBorrowed<'static> = CaseMapper::new();
const TITLECASE_MAPPER: TitlecaseMapperBorrowed<'static> = TitlecaseMapper::new();
const WORD_SEGMENTER: WordSegmenterBorrowed<'static> =
    WordSegmenter::new_for_non_complex_scripts(WordBreakInvariantOptions::default());
const GENERAL_CATEGORY: CodePointMapDataBorrowed<'static, GeneralCategory> =
    CodePointMapData::<GeneralCategory>::new();

/// The maximum number of bytes of preceding text kept as context for finding word boundaries.
const MAX_CONTEXT_LEN: usize = 32;

/// The case transform (and language) that applies to the text content of an element.
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
        let lang = LanguageIdentifier::try_from_str(&style.get_font()._x_lang.0)
            .unwrap_or(LanguageIdentifier::UNKNOWN);
        Self { kind, lang }
    }
}

/// Applies case transforms to the sequence of text nodes in an inline formatting context.
///
/// `capitalize` operates on words, which may span multiple text nodes, so the text already
/// pushed to the [`TreeBuilder`] is used as context for word segmentation. Only look-behind
/// is needed: whether a word boundary precedes a letter never depends on the text that
/// follows it.
#[derive(Default)]
pub(crate) struct TextTransformer {
    /// The offset in the builder's text of the last forced word boundary.
    context_start: usize,
}

impl TextTransformer {
    /// Forces a word boundary (e.g. at an atomic inline or a forced line break).
    pub(crate) fn word_break<B: Brush>(&mut self, builder: &mut TreeBuilder<'_, B>) {
        self.context_start = builder.text_so_far().text.len();
    }

    /// Transforms the content of a text node that is about to be pushed to `builder`.
    pub(crate) fn transform<'a, B: Brush>(
        &self,
        text: &'a str,
        transform: &CaseTransform,
        builder: &mut TreeBuilder<'_, B>,
    ) -> Cow<'a, str> {
        match transform.kind {
            TextTransform::UPPERCASE => CASE_MAPPER.uppercase_to_string(text, &transform.lang),
            TextTransform::LOWERCASE => CASE_MAPPER.lowercase_to_string(text, &transform.lang),
            TextTransform::CAPITALIZE => {
                let text_so_far = builder.text_so_far();
                let preceding = &text_so_far.text[self.context_start..];
                let start =
                    ceil_char_boundary(preceding, preceding.len().saturating_sub(MAX_CONTEXT_LEN));
                let mut context = Cow::Borrowed(&preceding[start..]);
                if text_so_far.pending_whitespace {
                    context.to_mut().push(' ');
                }
                Cow::Owned(capitalize(&context, text, &transform.lang))
            }
            _ => Cow::Borrowed(text),
        }
    }
}

/// Titlecases the first typographic letter unit of each word in `text`, leaving other
/// characters unchanged. `context` is the text preceding `text` in the same word segmentation
/// context.
fn capitalize(context: &str, text: &str, lang: &LanguageIdentifier) -> String {
    let mut options = TitlecaseOptions::default();
    options.leading_adjustment = Some(LeadingAdjustment::None);
    options.trailing_case = Some(TrailingCase::Unchanged);

    let context_len = context.len();
    let combined = [context, text].concat();

    let mut output = String::with_capacity(text.len());
    let mut segment_start = 0;
    for segment_end in WORD_SEGMENTER.segment_str(&combined).skip(1) {
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
                    output.push_str(&TITLECASE_MAPPER.titlecase_segment_to_string(
                        &part[letter_start..],
                        lang,
                        options,
                    ));
                }
                _ => output.push_str(part),
            }
        }
        segment_start = segment_end;
    }

    output
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
            lang: LanguageIdentifier::try_from_str(lang).unwrap(),
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
        transformer: &TextTransformer,
        text: &str,
        transform: &CaseTransform,
    ) -> String {
        let output = transformer.transform(text, transform, builder).into_owned();
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
    }

    #[test]
    fn capitalize_long_words() {
        let long_word = "a".repeat(100);
        let output = capitalize(&[&long_word, "bc de"]);
        assert_eq!(output[1], "bc De");
    }

    #[test]
    fn capitalize_language_sensitive() {
        assert_eq!(
            transform(TextTransform::CAPITALIZE, "nl", &["ijsland"]),
            ["IJsland"]
        );
    }

    #[test]
    fn uppercase_and_lowercase() {
        assert_eq!(
            transform(TextTransform::UPPERCASE, "und", &["straße"]),
            ["STRASSE"]
        );
        assert_eq!(transform(TextTransform::UPPERCASE, "tr", &["i"]), ["İ"]);
        assert_eq!(
            transform(TextTransform::LOWERCASE, "und", &["ABC"]),
            ["abc"]
        );
        assert_eq!(transform(TextTransform::LOWERCASE, "tr", &["I"]), ["ı"]);
    }
}
