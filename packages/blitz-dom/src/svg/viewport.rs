//! `viewBox` parsing.

/// Parse a `viewBox` attribute value: `<min-x> <min-y> <width> <height>`,
/// separated by ASCII whitespace and/or commas. `None` if it isn't exactly
/// 4 finite numbers.
pub(crate) fn parse_viewbox(value: &str) -> Option<[f64; 4]> {
    let mut numbers = value
        .split([' ', '\t', '\n', '\r', ','])
        .filter(|token| !token.is_empty())
        .map(|token| token.parse::<f64>().ok().filter(|n| n.is_finite()));

    let min_x = numbers.next()??;
    let min_y = numbers.next()??;
    let width = numbers.next()??;
    let height = numbers.next()??;
    if numbers.next().is_some() {
        return None; // trailing token after the 4 expected numbers
    }
    Some([min_x, min_y, width, height])
}

#[cfg(test)]
mod tests {
    use super::parse_viewbox;

    #[test]
    fn parses_whitespace_separated() {
        assert_eq!(parse_viewbox("0 0 100 100"), Some([0.0, 0.0, 100.0, 100.0]));
    }

    #[test]
    fn parses_comma_separated() {
        assert_eq!(parse_viewbox("0,0,485,58"), Some([0.0, 0.0, 485.0, 58.0]));
    }

    #[test]
    fn allows_negative_origin() {
        assert_eq!(
            parse_viewbox("-10 -5 200 100"),
            Some([-10.0, -5.0, 200.0, 100.0])
        );
    }

    #[test]
    fn rejects_wrong_count() {
        assert_eq!(parse_viewbox("0 0 100"), None);
        assert_eq!(parse_viewbox("0 0 100 100 100"), None);
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_viewbox("a b c d"), None);
        assert_eq!(parse_viewbox(""), None);
    }
}
