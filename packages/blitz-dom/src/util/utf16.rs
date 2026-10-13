//! Length computations for the HTML `maxlength` attribute, which measures a text control's
//! value in UTF-16 code units.

/// The prefix of `insertion` that fits when it replaces `selected` within `value` without
/// taking the value's [`value_length`] over `max_length`.
pub(crate) fn truncate_insertion<'a>(
    insertion: &'a str,
    value: &str,
    selected: &str,
    max_length: usize,
) -> &'a str {
    // A value's length never exceeds its UTF-8 byte length, so the byte lengths prove the
    // insertion fits without counting anything.
    if value.len() + insertion.len() <= max_length {
        return insertion;
    }
    let base_length = value_length(value).saturating_sub(value_length(selected));
    truncate_to_value_length(insertion, max_length.saturating_sub(base_length))
}

/// The length of a text control's value as measured by `maxlength`: UTF-16 code units, with
/// each CRLF pair counted once.
pub(crate) fn value_length(text: &str) -> usize {
    scan_value_length(text, usize::MAX).expect("usize::MAX cannot be exceeded")
}

/// The longest prefix of `text` whose [`value_length`] is at most `max_length`, never splitting
/// a character.
pub(crate) fn truncate_to_value_length(text: &str, max_length: usize) -> &str {
    match scan_value_length(text, max_length) {
        Ok(_) => text,
        Err(byte_idx) => &text[..byte_idx],
    }
}

/// Counts the [`value_length`] of `text` as long as it stays within `max_length` (both in
/// UTF-16 code units, with each CRLF pair counted once).
///
/// Returns `Ok(length)` if all of `text` fits, where `length` is its [`value_length`] in UTF-16
/// code units, or `Err(byte_idx)` where `byte_idx` is the UTF-8 byte index into `text` of the
/// first character that would take the length over `max_length`, so that `&text[..byte_idx]`
/// is the longest prefix that fits.
///
/// Works directly on the UTF-8 bytes without decoding: each non-continuation byte starts a
/// character worth one UTF-16 unit, or two for a 4-byte sequence (a surrogate pair).
fn scan_value_length(text: &str, max_length: usize) -> Result<usize, usize> {
    let mut length = 0;
    let mut prev = 0;
    for (byte_idx, byte) in text.bytes().enumerate() {
        let is_char_start = byte & 0xC0 != 0x80;
        let units =
            (is_char_start && !(byte == b'\n' && prev == b'\r')) as usize + (byte >= 0xF0) as usize;
        if length + units > max_length {
            return Err(byte_idx);
        }
        length += units;
        prev = byte;
    }
    Ok(length)
}

#[cfg(test)]
mod tests {
    use super::{truncate_insertion, truncate_to_value_length, value_length};

    #[test]
    fn value_length_counts_utf16_units_with_crlf_as_one() {
        for text in [
            "",
            "abc",
            "caf\u{e9}",
            "\u{20ac}",
            "\u{1F600}",
            "a\r\nb\n\r",
            "\r\n\r\n",
        ] {
            let expected = text.encode_utf16().count() - text.matches("\r\n").count();
            assert_eq!(value_length(text), expected, "{text:?}");
        }
    }

    #[test]
    fn truncate_never_splits_a_character() {
        assert_eq!(truncate_to_value_length("abc", 5), "abc");
        assert_eq!(truncate_to_value_length("abc", 0), "");
        assert_eq!(truncate_to_value_length("a\u{1F600}b", 2), "a");
        assert_eq!(truncate_to_value_length("a\u{1F600}b", 3), "a\u{1F600}");
        assert_eq!(truncate_to_value_length("a\r\nb", 2), "a\r\n");
        assert_eq!(truncate_to_value_length("a\r\nb", 1), "a");
    }

    #[test]
    fn truncate_insertion_accounts_for_the_replaced_selection() {
        assert_eq!(truncate_insertion("xyz", "ab", "", 3), "x");
        assert_eq!(truncate_insertion("xyz", "ab", "b", 3), "xy");
        assert_eq!(truncate_insertion("xyz", "abc", "", 3), "");
        assert_eq!(truncate_insertion("xyz", "abcd", "", 3), "");
        assert_eq!(truncate_insertion("\u{1F600}", "a", "", 2), "");
        assert_eq!(truncate_insertion("\u{1F600}", "a", "a", 2), "\u{1F600}");
    }
}
