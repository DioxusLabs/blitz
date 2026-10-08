//! Behavioural tests for `HTMLElement.innerText`, and its cost.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use blitz_dom::DocumentConfig;
use blitz_vibey_script::ScriptDocument;

/// The allocator, counting the allocations and the bytes each thread asks for.
struct Counting;

thread_local! {
    static ALLOCATIONS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|count| {
            let (allocations, bytes) = count.get();
            count.set((allocations + 1, bytes + layout.size()));
        });
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|count| {
            let (allocations, bytes) = count.get();
            count.set((allocations + 1, bytes + new_size));
        });
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// A document whose body holds `html`, laid out 600px wide.
fn document(html: &str) -> ScriptDocument {
    let html = format!(
        "<html><head><style>body {{ margin: 0; width: 600px; font: 16px sans-serif; }}</style></head><body>{html}</body></html>"
    );
    let mut doc = ScriptDocument::from_html(&html, DocumentConfig::default());
    doc.execute_scripts();
    doc
}

/// What the JavaScript expression `expression` gives, as a string.
fn eval(doc: &mut ScriptDocument, expression: &str) -> String {
    doc.eval(&format!("__blitz_send_message(String({expression}))"));
    let errors = doc.take_js_errors();
    assert!(errors.is_empty(), "{errors:?}");
    doc.take_messages()
        .pop()
        .expect("the expression's value was sent")
}

/// `innerText` of the element with id `id`, in a body holding `html`.
fn inner_text(html: &str, id: &str) -> String {
    eval(
        &mut document(html),
        &format!("document.getElementById({id:?}).innerText"),
    )
}

#[test]
fn nested_and_empty_inline_boxes() {
    let html = "<div id=t>a<span id=s>b<em>c</em><span></span>d</span><b></b>e</div>";
    assert_eq!(inner_text(html, "t"), "abcde");
    assert_eq!(inner_text(html, "s"), "bcd");
    let html = "<div id=t><span></span>a<span><span></span></span>b<span></span></div>";
    assert_eq!(inner_text(html, "t"), "ab");
}

#[test]
fn inline_block() {
    let html = "<div id=t>a <span style=\"display:inline-block\">b c</span> d</div>";
    assert_eq!(inner_text(html, "t"), "a b c d");
    let html = "<div id=t>a<span id=s style=\"display:inline-block\"><div>x</div><div>y</div></span>b</div>";
    assert_eq!(inner_text(html, "t"), "a\nx\ny\nb");
    assert_eq!(inner_text(html, "s"), "x\ny");
}

#[test]
fn line_breaks() {
    assert_eq!(
        inner_text("<div id=t>a<br>b<br><br>c</div>", "t"),
        "a\nb\n\nc"
    );
}

#[test]
fn block_inside_inline() {
    let html = "<div id=t>a<span id=s>b<div>c</div>d</span>e</div>";
    assert_eq!(inner_text(html, "t"), "ab\nc\nde");
    assert_eq!(inner_text(html, "s"), "b\nc\nd");
}

#[test]
fn display_none_children() {
    let html = "<div id=t>a<span style=\"display:none\">x</span>b<div style=\"display:none\">y</div>c</div>";
    assert_eq!(inner_text(html, "t"), "abc");
}

#[test]
fn absolutely_positioned_children() {
    let html = "<div id=t>a b<span style=\"position:absolute\">abs</span></div>";
    assert_eq!(inner_text(html, "t"), "a b\nabs");
    let html = "<div id=t>a <span style=\"position:absolute\">abs</span> b</div>";
    assert_eq!(inner_text(html, "t"), "a\nabs\n b");
    let html = "<div id=t style=\"width:40px\">aaa bbb <span style=\"position:absolute\">abs</span>ccc ddd</div>";
    assert_eq!(inner_text(html, "t"), "aaa bbb\nabs\n ccc ddd");
}

#[test]
fn floated_children() {
    let html = "<div id=t><span style=\"float:left\">flt</span>a b</div>";
    assert_eq!(inner_text(html, "t"), "flt\na b");
    let html = "<div id=t>a <span style=\"float:left\">flt</span> b</div>";
    assert_eq!(inner_text(html, "t"), "a\nflt\n b");
    let html = "<div>a <span id=s>b <span style=\"float:right\">flt</span> c</span> d</div>";
    assert_eq!(inner_text(html, "s"), "b\nflt\n c");
    let html =
        "<div id=t style=\"width:40px\">aaa bbb <span style=\"float:left\">flt</span>ccc ddd</div>";
    assert_eq!(inner_text(html, "t"), "aaa bbb\nflt\n ccc ddd");
}

#[test]
fn bidi_text() {
    let html = "<div id=t>abc <span>\u{5d0}\u{5d1}\u{5d2} \u{5d3}\u{5d4}</span> def</div>";
    assert_eq!(
        inner_text(html, "t"),
        "abc \u{5d0}\u{5d1}\u{5d2} \u{5d3}\u{5d4} def"
    );
    let html = "<div id=t dir=rtl>\u{5d0}\u{5d1} abc \u{5d2}\u{5d3}</div>";
    assert_eq!(inner_text(html, "t"), "\u{5d0}\u{5d1} abc \u{5d2}\u{5d3}");
}

#[test]
fn inline_box_split_across_lines() {
    let html = "<div id=t style=\"width:40px\">aaa <span id=s>bbb ccc ddd</span> eee</div>";
    assert_eq!(inner_text(html, "t"), "aaa bbb ccc ddd eee");
    assert_eq!(inner_text(html, "s"), "bbb ccc ddd");
    // The span has a box on each of three lines, and its offset is its first box's.
    let geometry = eval(
        &mut document(html),
        "(() => { const s = document.getElementById('s'); const r = s.getClientRects(); \
         return [r.length, r[0].top, r[0].left, s.offsetTop, s.offsetLeft].join(); })()",
    );
    assert_eq!(geometry, "3,18,0,18,0");
}

#[test]
fn white_space_collapsing() {
    let html = "<div id=t>   a   b  <span>  c </span>  </div>";
    assert_eq!(inner_text(html, "t"), "a b c");
    let html = "<div id=t>  <p>  a  </p>  b  <div> c </div>  </div>";
    assert_eq!(inner_text(html, "t"), "a\n\nb\nc");
}

/// The allocations and bytes that reading `innerText` of `#t` asks for, once layout is done, and
/// the length of the text.
fn inner_text_cost(spans: usize) -> (usize, usize, usize) {
    let html = format!("<div id=t>{}</div>", "<span>word</span> ".repeat(spans));
    let mut doc = document(&html);
    // The first read lays the document out.
    eval(&mut doc, "document.getElementById('t').innerText");
    doc.eval("globalThis.t = document.getElementById('t'); globalThis.text = ''");
    let before = ALLOCATIONS.with(Cell::get);
    doc.eval("text = t.innerText");
    let after = ALLOCATIONS.with(Cell::get);
    let len = eval(&mut doc, "text.length").parse().unwrap();
    (after.0 - before.0, after.1 - before.1, len)
}

/// `innerText` of a long paragraph of many spans allocates nothing per cluster or span: four
/// times the spans ask for about the same number of allocations, and for no more bytes than
/// a few copies of the longer text.
#[test]
fn inner_text_does_not_allocate_per_cluster() {
    let (allocations, bytes, len) = inner_text_cost(500);
    let (allocations_4, bytes_4, len_4) = inner_text_cost(2000);
    assert!(len_4 > len);
    assert!(
        allocations_4 <= allocations + 16,
        "{allocations} allocations for 500 spans, {allocations_4} for 2000"
    );
    let extra = bytes_4.saturating_sub(bytes);
    assert!(
        extra <= 16 * (len_4 - len),
        "{bytes} bytes for 500 spans, {bytes_4} for 2000"
    );
}
