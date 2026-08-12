//! `viewBox` + `preserveAspectRatio` parsing and the viewBox->viewport [`Affine`] they produce.

use kurbo::{Affine, Rect, Size};

use super::context::{Align, MeetOrSlice, PreserveAspectRatio};

/// A minimal SVG number-list tokenizer: splits on ASCII whitespace and/or
/// commas (SVG's "wsp* comma? wsp*" separator grammar), parsing each token
/// as an `f64`. Shared by `viewBox` and (in `geometry.rs`) `points`.
pub(super) fn parse_number_list(s: &str) -> Vec<f64> {
    s.split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|tok| !tok.is_empty())
        .filter_map(|tok| tok.parse::<f64>().ok())
        .collect()
}

/// Parse a `viewBox="min-x min-y width height"` attribute value. Returns `None` if malformed,
/// the caller then treats the `<svg>` as if it had no `viewBox` at all, which is the closest
/// interoperable fallback.
pub fn parse_viewbox(value: &str) -> Option<Rect> {
    let nums = parse_number_list(value);
    if nums.len() != 4 {
        return None;
    }
    let (min_x, min_y, w, h) = (nums[0], nums[1], nums[2], nums[3]);
    Some(Rect::new(min_x, min_y, min_x + w, min_y + h))
}

/// Parse a `preserveAspectRatio="[defer] <align> [<meetOrSlice>]"` value. `defer` is accepted
/// and ignored. Falls back to the SVG2 default (`xMidYMid meet`) on any unrecognized token,
/// error-recovery for presentation attributes.
pub fn parse_preserve_aspect_ratio(value: &str) -> PreserveAspectRatio {
    let mut tokens = value.split_ascii_whitespace().peekable();
    if tokens.peek() == Some(&"defer") {
        tokens.next();
    }

    let align = match tokens.next() {
        Some("none") => Align::None,
        Some("xMinYMin") => Align::XMinYMin,
        Some("xMidYMin") => Align::XMidYMin,
        Some("xMaxYMin") => Align::XMaxYMin,
        Some("xMinYMid") => Align::XMinYMid,
        Some("xMidYMid") | None => Align::XMidYMid,
        Some("xMaxYMid") => Align::XMaxYMid,
        Some("xMinYMax") => Align::XMinYMax,
        Some("xMidYMax") => Align::XMidYMax,
        Some("xMaxYMax") => Align::XMaxYMax,
        Some(_) => Align::XMidYMid,
    };

    let meet_or_slice = match tokens.next() {
        Some("slice") => MeetOrSlice::Slice,
        _ => MeetOrSlice::Meet,
    };

    PreserveAspectRatio {
        align,
        meet_or_slice,
    }
}

/// Compute the `viewBox` coordinate space -> viewport (CSS box) space transform.
/// `viewbox.width == 0 || viewbox.height == 0` is the caller's responsibility to check first.
pub fn viewbox_to_viewport_ctm(viewbox: Rect, viewport: Size, par: PreserveAspectRatio) -> Affine {
    let vb_w = viewbox.width();
    let vb_h = viewbox.height();

    let (sx, sy) = if par.align == Align::None {
        (viewport.width / vb_w, viewport.height / vb_h)
    } else {
        let sx = viewport.width / vb_w;
        let sy = viewport.height / vb_h;
        let s = match par.meet_or_slice {
            MeetOrSlice::Meet => sx.min(sy),
            MeetOrSlice::Slice => sx.max(sy),
        };
        (s, s)
    };

    let (align_x, align_y) = align_fractions(par.align);
    let tx = -viewbox.x0 * sx + align_x * (viewport.width - vb_w * sx);
    let ty = -viewbox.y0 * sy + align_y * (viewport.height - vb_h * sy);

    Affine::new([sx, 0.0, 0.0, sy, tx, ty])
}

/// `viewBox` absent -> identity CTM, viewport equals the CSS box directly.
pub fn identity_ctm() -> Affine {
    Affine::IDENTITY
}

fn align_fractions(align: Align) -> (f64, f64) {
    let x = match align {
        Align::None | Align::XMinYMin | Align::XMinYMid | Align::XMinYMax => 0.0,
        Align::XMidYMin | Align::XMidYMid | Align::XMidYMax => 0.5,
        Align::XMaxYMin | Align::XMaxYMid | Align::XMaxYMax => 1.0,
    };
    let y = match align {
        Align::None | Align::XMinYMin | Align::XMidYMin | Align::XMaxYMin => 0.0,
        Align::XMinYMid | Align::XMidYMid | Align::XMaxYMid => 0.5,
        Align::XMinYMax | Align::XMidYMax | Align::XMaxYMax => 1.0,
    };
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Point;

    #[test]
    fn parses_viewbox_with_commas_and_whitespace() {
        assert_eq!(
            parse_viewbox("0 0 200 100"),
            Some(Rect::new(0.0, 0.0, 200.0, 100.0))
        );
        assert_eq!(
            parse_viewbox("0,0,200,100"),
            Some(Rect::new(0.0, 0.0, 200.0, 100.0))
        );
        assert_eq!(
            parse_viewbox("-10 -5 220 110"),
            Some(Rect::new(-10.0, -5.0, 210.0, 105.0))
        );
    }

    #[test]
    fn rejects_malformed_viewbox() {
        assert_eq!(parse_viewbox("0 0 200"), None);
        assert_eq!(parse_viewbox("not a viewbox"), None);
    }

    #[test]
    fn default_preserve_aspect_ratio_is_xmidymid_meet() {
        let par = parse_preserve_aspect_ratio("");
        assert_eq!(par.align, Align::XMidYMid);
        assert_eq!(par.meet_or_slice, MeetOrSlice::Meet);
    }

    #[test]
    fn parses_none_and_slice() {
        let par = parse_preserve_aspect_ratio("none");
        assert_eq!(par.align, Align::None);
        let par = parse_preserve_aspect_ratio("xMinYMax slice");
        assert_eq!(par.align, Align::XMinYMax);
        assert_eq!(par.meet_or_slice, MeetOrSlice::Slice);
    }

    #[test]
    fn defer_prefix_is_ignored() {
        let par = parse_preserve_aspect_ratio("defer xMaxYMax meet");
        assert_eq!(par.align, Align::XMaxYMax);
    }

    #[test]
    fn meet_contain_scales_uniformly_and_centers() {
        // viewBox 0 0 100 50, viewport 200 200: meet picks min(sx=2, sy=4) = 2, centered on the shorter axis.
        let vb = Rect::new(0.0, 0.0, 100.0, 50.0);
        let vp = Size::new(200.0, 200.0);
        let ctm = viewbox_to_viewport_ctm(vb, vp, PreserveAspectRatio::default());
        assert_eq!(ctm * Point::new(0.0, 0.0), Point::new(0.0, 50.0));
        assert_eq!(ctm * Point::new(100.0, 50.0), Point::new(200.0, 150.0));
    }

    #[test]
    fn none_align_scales_non_uniformly() {
        let vb = Rect::new(0.0, 0.0, 100.0, 50.0);
        let vp = Size::new(200.0, 200.0);
        let par = PreserveAspectRatio {
            align: Align::None,
            meet_or_slice: MeetOrSlice::Meet,
        };
        let ctm = viewbox_to_viewport_ctm(vb, vp, par);
        assert_eq!(ctm * Point::new(100.0, 50.0), Point::new(200.0, 200.0));
    }

    #[test]
    fn slice_cover_scales_uniformly_and_overflows() {
        let vb = Rect::new(0.0, 0.0, 100.0, 50.0);
        let vp = Size::new(200.0, 200.0);
        let par = PreserveAspectRatio {
            align: Align::XMidYMid,
            meet_or_slice: MeetOrSlice::Slice,
        };
        let ctm = viewbox_to_viewport_ctm(vb, vp, par);
        // slice picks max(sx=2, sy=4) = 4; viewBox width*4 = 400 > viewport width 200, so it overflows.
        assert_eq!(ctm * Point::new(100.0, 0.0), Point::new(400.0 - 100.0, 0.0));
    }
}
