//! Ruby annotations pair with bases as CSS Ruby pairs them, and an annotation whose parent is no
//! ruby container is wrapped in an anonymous one (CSS Ruby 1, sections 2.2 and 2.3).
//!
//! They read the client rects of the bases and annotations winkin lays out.

blitz_dom::cfg_text_backend! {
    parley => {}
    winkin => {
        use blitz_dom::{BaseDocument, BoundingRect};
        use blitz_test_harness::Harness;

        fn rect(doc: &BaseDocument, selector: &str) -> BoundingRect {
            let id = doc.query_selector(selector).unwrap().unwrap();
            let rects = doc.node_client_rects(id);
            assert_eq!(rects.len(), 1, "{selector}: {rects:?}");
            rects[0]
        }

        fn center(rect: BoundingRect) -> f64 {
            rect.x + rect.width / 2.0
        }

        fn page(body: &str) -> Harness {
            Harness::from_html(&format!(
                "<style>body {{ margin: 0; font-size: 40px; line-height: 1 }} \
                 rt {{ font-size: 20px }}</style><p>{body}</p>"
            ))
        }

        /// `<rb>a</rb><rb>b</rb><rt>x</rt><rt>y</rt>`: each annotation goes over its own base,
        /// where Chrome sets the first over both.
        #[test]
        fn each_annotation_pairs_with_its_own_base() {
            let harness = page(
                "<ruby><rb id=a>a</rb><rb id=b>b</rb><rt id=x>x</rt><rt id=y>y</rt></ruby>",
            );
            let doc = harness.base();
            let (a, b, x, y) = (rect(&doc, "#a"), rect(&doc, "#b"), rect(&doc, "#x"), rect(&doc, "#y"));
            assert!((center(x) - center(a)).abs() < 0.5, "{a:?} {x:?}");
            assert!((center(y) - center(b)).abs() < 0.5, "{b:?} {y:?}");
            assert_eq!(x.y, y.y);
        }

        /// Two `<rt>`s after one base sit side by side on one level, the second over an empty
        /// base, as in CSS and Chrome.
        #[test]
        fn two_annotations_after_one_base_sit_side_by_side() {
            let harness = page("<ruby><span id=a>A</span><rt id=x>XX</rt><rt id=y>yy</rt></ruby>");
            let doc = harness.base();
            let (a, x, y) = (rect(&doc, "#a"), rect(&doc, "#x"), rect(&doc, "#y"));
            assert_eq!(x.y, y.y);
            assert!((center(x) - center(a)).abs() < 0.5, "{a:?} {x:?}");
            assert!(y.x >= x.x + x.width - 0.5, "{x:?} {y:?}");
        }

        /// Annotations in two `<rtc>`s pair with the bases level by level, and white space between
        /// the bases and between the annotations pairs as a column of its own.
        #[test]
        fn annotation_containers_pair_level_by_level() {
            let harness = page(
                "<ruby><rb id=a>a</rb> <rb id=b>b</rb>\
                 <rtc><rt id=x>x</rt> <rt id=y>y</rt></rtc>\
                 <rtc><rt id=p>p</rt> <rt id=q>q</rt></rtc></ruby>",
            );
            let doc = harness.base();
            let (a, b) = (rect(&doc, "#a"), rect(&doc, "#b"));
            let (x, y, p, q) = (rect(&doc, "#x"), rect(&doc, "#y"), rect(&doc, "#p"), rect(&doc, "#q"));
            assert!(b.x > a.x + a.width, "{a:?} {b:?}");
            for (base, over) in [(a, x), (b, y), (a, p), (b, q)] {
                assert!((center(over) - center(base)).abs() < 0.5, "{base:?} {over:?}");
            }
            assert_eq!(x.y, y.y);
            assert_eq!(p.y, q.y);
            assert!(p.y < x.y, "{x:?} {p:?}");
        }

        /// An `<rt>` inside a span inside a ruby goes in an anonymous ruby container there, over
        /// an empty base, rather than over the base before it.
        #[test]
        fn an_annotation_in_a_span_is_wrapped_in_its_own_ruby() {
            let harness = page(
                "<ruby><span><span id=a>A</span><rt id=x>XX</rt><span id=b>B</span></span></ruby>",
            );
            let doc = harness.base();
            let (a, b, x) = (rect(&doc, "#a"), rect(&doc, "#b"), rect(&doc, "#x"));
            // Over the line, though its line height reaches into the base's content area.
            assert!(x.y + x.height / 2.0 < b.y, "{x:?} {b:?}");
            assert!(x.x >= a.x + a.width - 0.5, "{a:?} {x:?}");
            assert!((b.x - (x.x + x.width)).abs() < 0.5, "{x:?} {b:?}");
        }
    }
}
