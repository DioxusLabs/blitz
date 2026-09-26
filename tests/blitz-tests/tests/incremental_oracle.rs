//! Equivalence oracle between incremental and non-incremental layout.
//!
//! Non-incremental mode is defined as "every node is damaged every frame" and
//! runs through the same damage pipeline as incremental mode, so the two must
//! produce identical layouts after any sequence of mutations. Each scenario
//! below runs the same fixture + mutation sequence through one document of
//! each mode and compares every node's `final_layout()` after each step.
//!
//! Comparison walks the DOM tree by tree position (children index and
//! `::before`/`::after`) since node ids are not guaranteed to match across the
//! two documents. Anonymous blocks exist only in the layout tree, so at every
//! node the `layout_children` lists are additionally compared positionally
//! (length + each child's layout, recursing into anonymous children), which
//! covers them without relying on stable ids.
//!
//! The paint tree is compared the same way: each node's `paint_children` as
//! indices into its `layout_children`, and each stacking-context root's
//! hoisted children as (layout path from the root, z-index) pairs. This is
//! the main guard for the clean-subtree skip in `build_paint_tree`.

use blitz_dom::{DocumentConfig, LocalName, QualName, ns};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::node_id::NodeId;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;

fn qname(local: &str) -> QualName {
    QualName {
        prefix: None,
        ns: ns!(html),
        local: LocalName::from(local),
    }
}

fn attr(name: &str) -> QualName {
    QualName::new(None, ns!(), name.into())
}

fn make_doc(html: &str, incremental: bool) -> HtmlDocument {
    let mut doc = HtmlDocument::from_html(
        html,
        DocumentConfig {
            viewport: Some(Viewport::new(400, 400, 1.0, ColorScheme::Light)),
            html_parser_provider: Some(Arc::new(HtmlProvider) as _),
            incremental: Some(incremental),
            ..Default::default()
        },
    );
    doc.resolve(0.0);
    doc
}

fn id(doc: &HtmlDocument, selector: &str) -> NodeId {
    doc.query_selector(selector)
        .unwrap()
        .unwrap_or_else(|| panic!("no node matches {selector}"))
}

fn describe(doc: &HtmlDocument, node_id: NodeId) -> String {
    let node = doc.get_node(node_id).unwrap();
    if node.is_anonymous() {
        "<anon>".to_string()
    } else if let Some(el) = node.element_data() {
        match el.id.as_ref() {
            Some(id) => format!("<{} id={}>", el.name.local, id),
            None => format!("<{}>", el.name.local),
        }
    } else if node.text_data().is_some() {
        "#text".to_string()
    } else {
        format!("{:?}", node.data.kind())
    }
}

/// Location and size of the node's box. Text and comment nodes do not
/// participate in layout (their text is laid out by the enclosing inline
/// root, whose box is compared instead) and yield `None`.
fn layout_of(doc: &HtmlDocument, node_id: NodeId) -> Option<(f32, f32, f32, f32)> {
    let node = doc.get_node(node_id).unwrap();
    if node.element_data().is_none() && !node.is_anonymous() {
        return None;
    }
    let l = node.final_layout();
    Some((l.location.x, l.location.y, l.size.width, l.size.height))
}

fn assert_node_eq(
    inc: &HtmlDocument,
    inc_id: NodeId,
    non: &HtmlDocument,
    non_id: NodeId,
    path: &str,
    step: &str,
) {
    assert_eq!(
        describe(inc, inc_id),
        describe(non, non_id),
        "[{step}] node kind mismatch at {path}"
    );
    assert_eq!(
        layout_of(inc, inc_id),
        layout_of(non, non_id),
        "[{step}] layout mismatch at {path} ({})",
        describe(inc, inc_id)
    );
}

/// Index of `child` in `parent`'s `layout_children`.
fn layout_index(doc: &HtmlDocument, parent: NodeId, child: NodeId) -> usize {
    doc.get_node(parent)
        .unwrap()
        .layout_children
        .borrow()
        .as_ref()
        .and_then(|children| children.iter().position(|id| *id == child))
        .unwrap_or_else(|| {
            panic!(
                "{} is not a layout child of {}",
                describe(doc, child),
                describe(doc, parent)
            )
        })
}

/// Layout-tree path (child indices) from `ancestor` down to `node`.
fn layout_path(doc: &HtmlDocument, ancestor: NodeId, node: NodeId) -> Vec<usize> {
    let mut path = Vec::new();
    let mut current = node;
    while current != ancestor {
        let parent = doc
            .get_node(current)
            .unwrap()
            .layout_parent
            .get()
            .unwrap_or_else(|| {
                panic!(
                    "{} is not a layout descendant of {}",
                    describe(doc, node),
                    describe(doc, ancestor)
                )
            });
        path.push(layout_index(doc, parent, current));
        current = parent;
    }
    path.reverse();
    path
}

/// Hoisted stacking-context entries as (layout path, z-index) pairs.
type HoistedEntries = Vec<(Vec<usize>, i32)>;

/// Positional description of a node's paint tree: its `paint_children` as
/// layout paths (length 1 for direct children, longer for out-of-flow boxes
/// owned by this node as their containing block), and (if it is a
/// stacking-context root) its hoisted children as (layout path, z-index) pairs.
fn paint_tree_of(doc: &HtmlDocument, node_id: NodeId) -> (Vec<Vec<usize>>, Option<HoistedEntries>) {
    let node = doc.get_node(node_id).unwrap();
    let paint_children = node
        .paint_children
        .borrow()
        .iter()
        .flatten()
        .map(|child| layout_path(doc, node_id, *child))
        .collect();
    let hoisted = node.stacking_context.as_ref().map(|sc| {
        sc.children
            .iter()
            .map(|child| (layout_path(doc, node_id, child.node_id), child.z_index))
            .collect()
    });
    (paint_children, hoisted)
}

/// Compare the layout trees (`layout_children`, which includes anonymous
/// boxes) rooted at the two nodes positionally, along with their paint trees.
fn compare_layout_tree(
    inc: &HtmlDocument,
    inc_id: NodeId,
    non: &HtmlDocument,
    non_id: NodeId,
    path: &str,
    step: &str,
) {
    compare_layout_tree_inner(inc, inc_id, non, non_id, path, step);
    assert_eq!(
        paint_tree_of(inc, inc_id),
        paint_tree_of(non, non_id),
        "[{step}] paint tree mismatch at {path} ({})",
        describe(inc, inc_id)
    );
}

fn compare_layout_tree_inner(
    inc: &HtmlDocument,
    inc_id: NodeId,
    non: &HtmlDocument,
    non_id: NodeId,
    path: &str,
    step: &str,
) {
    assert_node_eq(inc, inc_id, non, non_id, path, step);

    let inc_children = inc
        .get_node(inc_id)
        .unwrap()
        .layout_children
        .borrow()
        .clone()
        .unwrap_or_default();
    let non_children = non
        .get_node(non_id)
        .unwrap()
        .layout_children
        .borrow()
        .clone()
        .unwrap_or_default();
    assert_eq!(
        inc_children.len(),
        non_children.len(),
        "[{step}] layout_children length mismatch at {path}: {:?} vs {:?}",
        inc_children
            .iter()
            .map(|id| describe(inc, *id))
            .collect::<Vec<_>>(),
        non_children
            .iter()
            .map(|id| describe(non, *id))
            .collect::<Vec<_>>(),
    );
    for (i, (a, b)) in inc_children.iter().zip(non_children.iter()).enumerate() {
        compare_layout_tree(inc, *a, non, *b, &format!("{path}/L{i}"), step);
    }
}

/// Compare the DOM trees rooted at the two nodes positionally, and at each
/// node also compare its layout tree.
fn compare_dom_tree(
    inc: &HtmlDocument,
    inc_id: NodeId,
    non: &HtmlDocument,
    non_id: NodeId,
    path: &str,
    step: &str,
) {
    compare_layout_tree(inc, inc_id, non, non_id, path, step);

    let inc_node = inc.get_node(inc_id).unwrap();
    let non_node = non.get_node(non_id).unwrap();

    assert_eq!(
        inc_node.children.len(),
        non_node.children.len(),
        "[{step}] DOM children length mismatch at {path}"
    );
    for (i, (a, b)) in inc_node
        .children
        .iter()
        .zip(non_node.children.iter())
        .enumerate()
    {
        compare_dom_tree(inc, *a, non, *b, &format!("{path}/{i}"), step);
    }

    for (name, a, b) in [
        ("::before", inc_node.before(), non_node.before()),
        ("::after", inc_node.after(), non_node.after()),
    ] {
        assert_eq!(
            a.is_some(),
            b.is_some(),
            "[{step}] {name} presence mismatch at {path}"
        );
        if let (Some(a), Some(b)) = (a, b) {
            compare_dom_tree(inc, a, non, b, &format!("{path}/{name}"), step);
        }
    }
}

/// Walk DOM children, layout children (anonymous boxes) and pseudo-elements,
/// asserting that no node retains damage after a resolve.
fn assert_no_damage(doc: &HtmlDocument, step: &str) {
    fn walk(doc: &HtmlDocument, node_id: NodeId, step: &str) {
        let node = doc.get_node(node_id).unwrap();
        if let Some(damage) = node.damage() {
            assert!(
                damage.is_empty(),
                "[{step}] node {} retains damage {damage:?} after resolve",
                describe(doc, node_id)
            );
        }
        assert!(
            !node.has_damaged_descendants(),
            "[{step}] node {} retains damaged_descendants after resolve",
            describe(doc, node_id)
        );
        for child in node.children.iter() {
            walk(doc, *child, step);
        }
        if let Some(children) = node.layout_children.borrow().as_ref() {
            for child in children.iter() {
                walk(doc, *child, step);
            }
        }
        if let Some(before) = node.before() {
            walk(doc, before, step);
        }
        if let Some(after) = node.after() {
            walk(doc, after, step);
        }
    }
    walk(doc, doc.root_node().id, step);
}

struct Oracle {
    inc: HtmlDocument,
    non: HtmlDocument,
}

impl Oracle {
    fn new(html: &str) -> Self {
        let oracle = Self {
            inc: make_doc(html, true),
            non: make_doc(html, false),
        };
        oracle.check("initial");
        oracle
    }

    fn check(&self, step: &str) {
        assert!(self.inc.incremental_layout());
        assert!(!self.non.incremental_layout());
        let inc_root = self.inc.root_element().id;
        let non_root = self.non.root_element().id;
        compare_dom_tree(&self.inc, inc_root, &self.non, non_root, "html", step);
        assert_no_damage(&self.inc, step);
        assert_no_damage(&self.non, step);
    }

    /// Apply the same mutation to both documents, resolve, and compare.
    fn step(&mut self, name: &str, mutation: impl Fn(&mut HtmlDocument)) {
        for doc in [&mut self.inc, &mut self.non] {
            mutation(doc);
            doc.resolve(0.0);
        }
        self.check(name);
    }
}

fn set_style(doc: &mut HtmlDocument, selector: &str, style: &str) {
    let node_id = id(doc, selector);
    doc.mutate().set_attribute(node_id, attr("style"), style);
}

/// Center of the given element (for hover).
fn center(doc: &HtmlDocument, selector: &str) -> (f32, f32) {
    let node = doc.get_node(id(doc, selector)).unwrap();
    let l = node.final_layout();
    // Walk up to compute the absolute position.
    let mut x = l.location.x + l.size.width / 2.0;
    let mut y = l.location.y + l.size.height / 2.0;
    let mut current = node.layout_parent.get();
    while let Some(parent_id) = current {
        let parent = doc.get_node(parent_id).unwrap();
        x += parent.final_layout().location.x;
        y += parent.final_layout().location.y;
        current = parent.layout_parent.get();
    }
    (x, y)
}

const FIXTURE: &str = r#"<html><head><style>
    body { margin: 0; font-size: 10px; line-height: 1; font-family: sans-serif; }
    #hover-target { width: 40px; height: 20px; background: red; }
    #hover-target:hover { height: 50px; }
    #pseudo::before { content: "b"; display: inline-block; width: 15px; height: 15px; }
    #pseudo::after { content: "a"; display: block; height: 7px; }
    .contents { display: contents; }
    #z1:hover { z-index: 5; }
    #z2:hover { position: static; }
    #sc:hover { opacity: 1; }
    #fz:hover { z-index: 0; }
    #c2:hover { background: blue; }
    #abs:hover { top: 15px; }
</style></head><body>
    <div id="block" style="width: 300px;">
        <div id="hover-target"></div>
        <div id="inline-root">
            text <span id="ib" style="display:inline-block; width:50px; height:20px;"></span> more text
            <div id="nested-block" style="height: 12px;"></div>
            tail <span id="span">span text</span>
        </div>
        <div id="pseudo">pseudo host</div>
        <div class="contents" id="contents">
            <div id="c1" style="height: 8px;"></div>
            <div id="c2" style="height: 9px;"></div>
        </div>
    </div>
    <div id="flex" style="display:flex; width: 300px;">
        <div id="fa" style="width:10px; height:10px; order: 1"></div>
        <div id="fb" style="width:10px; height:10px;"></div>
        <div id="fc" style="width:10px; height:10px; order: -1"></div>
        <div id="fz" style="width:10px; height:10px; z-index: 2"></div>
        flex text
    </div>
    <div id="grid" style="display:grid; grid-template-columns: 20px 20px; grid-auto-rows: 10px; width: 300px;">
        <div id="ga" style="order: 2"></div>
        <div id="gb"></div>
        <div id="gc" style="order: -1"></div>
    </div>
    <div id="rel" style="position: relative; height: 30px;">
        <div id="abs" style="position: absolute; top: 5px; left: 7px; width: 11px; height: 13px;"></div>
        <div id="fixed" style="position: fixed; top: 100px; left: 100px; width: 20px; height: 20px;"></div>
        <div id="neg" style="position: relative; z-index: -1; height: 4px;"></div>
        <div id="sc" style="opacity: 0.5; height: 20px;">
            <div id="deep"><div id="z1" style="position: relative; z-index: 1; height: 4px;"></div>
                text <span id="zi" style="position: relative; z-index: 3;">inline</span></div>
            <div id="z2" style="position: relative; z-index: 2; height: 4px;"></div>
        </div>
    </div>
</body></html>"#;

#[test]
fn restyle_via_hover() {
    let mut oracle = Oracle::new(FIXTURE);

    let (x, y) = center(&oracle.inc, "#hover-target");
    oracle.step("hover on", |doc| {
        assert!(doc.set_hover_to(x, y));
    });
    assert_eq!(
        oracle.inc.get_hover_node_id(),
        Some(id(&oracle.inc, "#hover-target"))
    );
    assert_eq!(
        layout_of(&oracle.inc, id(&oracle.inc, "#hover-target"))
            .unwrap()
            .3,
        50.0
    );

    oracle.step("hover off", |doc| {
        assert!(doc.set_hover_to(390.0, 390.0));
    });
    assert_eq!(
        layout_of(&oracle.inc, id(&oracle.inc, "#hover-target"))
            .unwrap()
            .3,
        20.0
    );
}

#[test]
fn style_attribute_width_display_order() {
    let mut oracle = Oracle::new(FIXTURE);

    oracle.step("width", |doc| set_style(doc, "#block", "width: 200px;"));

    for display in ["inline", "none", "flex", "block", "inline", "block"] {
        oracle.step(&format!("nested-block display:{display}"), |doc| {
            set_style(
                doc,
                "#nested-block",
                &format!("height: 12px; display: {display};"),
            )
        });
    }
    for display in ["none", "inline", "block", "flex"] {
        oracle.step(&format!("hover-target display:{display}"), |doc| {
            set_style(doc, "#hover-target", &format!("display: {display};"))
        });
    }

    oracle.step("flex order", |doc| {
        set_style(doc, "#fb", "width:10px; height:10px; order: 5;")
    });
    oracle.step("grid order", |doc| set_style(doc, "#gb", "order: -3;"));
    oracle.step("flex order reset", |doc| {
        set_style(doc, "#fb", "width:10px; height:10px;")
    });
}

/// Build a subtree containing inline text and a block: `<div id=..>hello <b>x</b><div style=height:6px></div> world</div>`
fn build_subtree(doc: &mut HtmlDocument, subtree_id: &str) -> NodeId {
    let mut m = doc.mutate();
    let wrapper = m.create_element(qname("div"), Vec::new());
    m.set_attribute(wrapper, attr("id"), subtree_id);
    let t1 = m.create_text_node("hello ");
    let b = m.create_element(qname("b"), Vec::new());
    let bt = m.create_text_node("x");
    m.append_children(b, &[bt]);
    let inner = m.create_element(qname("div"), Vec::new());
    m.set_attribute(inner, attr("style"), "height: 6px;");
    let t2 = m.create_text_node(" world");
    m.append_children(wrapper, &[t1, b, inner, t2]);
    wrapper
}

#[test]
fn append_remove_insert_subtree() {
    let mut oracle = Oracle::new(FIXTURE);

    oracle.step("append to block", |doc| {
        let subtree = build_subtree(doc, "appended");
        let parent = id(doc, "#block");
        doc.mutate().append_children(parent, &[subtree]);
    });

    oracle.step("append into inline root", |doc| {
        let subtree = build_subtree(doc, "appended-inline");
        let parent = id(doc, "#inline-root");
        doc.mutate().append_children(parent, &[subtree]);
    });

    oracle.step("insert before span", |doc| {
        let subtree = build_subtree(doc, "inserted");
        let anchor = id(doc, "#span");
        doc.mutate().insert_nodes_before(anchor, &[subtree]);
    });

    oracle.step("insert before flex item", |doc| {
        let subtree = build_subtree(doc, "inserted-flex");
        let anchor = id(doc, "#fb");
        doc.mutate().insert_nodes_before(anchor, &[subtree]);
    });

    oracle.step("remove inserted", |doc| {
        let node = id(doc, "#inserted");
        doc.mutate().remove_node(node);
    });
    oracle.step("remove appended", |doc| {
        let node = id(doc, "#appended");
        doc.mutate().remove_node(node);
    });
    oracle.step("remove nested block", |doc| {
        let node = id(doc, "#nested-block");
        doc.mutate().remove_node(node);
    });
    oracle.step("remove display:contents child", |doc| {
        let node = id(doc, "#c1");
        doc.mutate().remove_node(node);
    });
    oracle.step("remove abspos", |doc| {
        let node = id(doc, "#abs");
        doc.mutate().remove_node(node);
    });
}

#[test]
fn set_inner_html() {
    let mut oracle = Oracle::new(FIXTURE);

    oracle.step("inner html of block", |doc| {
        let node = id(doc, "#inline-root");
        doc.mutate().set_inner_html(
            node,
            r#"new <em>inline</em> content <div style="height: 4px"></div> and more"#,
        );
    });
    oracle.step("inner html of flex", |doc| {
        let node = id(doc, "#flex");
        doc.mutate().set_inner_html(
            node,
            r#"<div style="width: 30px; height: 5px; order: 1"></div>t<div style="width: 30px; height: 5px;"></div>"#,
        );
    });
    oracle.step("inner html of pseudo host", |doc| {
        let node = id(doc, "#pseudo");
        doc.mutate().set_inner_html(node, "replaced");
    });
    oracle.step("empty inner html", |doc| {
        let node = id(doc, "#block");
        doc.mutate().set_inner_html(node, "");
    });
}

#[test]
fn set_node_text_in_inline_root() {
    let mut oracle = Oracle::new(FIXTURE);

    for text in [
        "much longer span text which should wrap onto several lines of the inline root",
        "",
        "short",
    ] {
        oracle.step(&format!("set_node_text {text:?}"), |doc| {
            let span = id(doc, "#span");
            let text_node = doc.get_node(span).unwrap().children[0];
            doc.mutate().set_node_text(text_node, text);
        });
    }

    oracle.step("set_node_text on bare inline text", |doc| {
        let root = id(doc, "#inline-root");
        let text_node = doc.get_node(root).unwrap().children[0];
        doc.mutate()
            .set_node_text(text_node, "replaced leading text ");
    });
}

/// Style changes which alter paint ownership (z-index, position, becoming
/// or ceasing to be a stacking-context root) interleaved with changes which
/// do not (colour, geometry) so that clean subtrees are replayed.
#[test]
fn paint_tree_z_index_and_stacking_contexts() {
    let mut oracle = Oracle::new(FIXTURE);

    // Hover-driven restyles produce the minimal damage for each property
    // (style attribute mutations below rebuild the box), so these exercise
    // the clean-subtree skip and replay.
    for selector in ["#c2", "#z1", "#z2", "#sc", "#fz", "#abs", "#c2"] {
        let (x, y) = center(&oracle.inc, selector);
        oracle.step(&format!("hover {selector}"), |doc| {
            assert!(doc.set_hover_to(x, y));
        });
        let target = id(&oracle.inc, selector);
        let mut hovered = oracle.inc.get_hover_node_id();
        while let Some(node_id) = hovered
            && node_id != target
        {
            hovered = oracle.inc.get_node(node_id).unwrap().parent;
        }
        assert_eq!(hovered, Some(target), "hover {selector}");
        oracle.step(&format!("unhover {selector}"), |doc| {
            assert!(doc.set_hover_to(390.0, 390.0));
        });
    }

    oracle.step("colour only", |doc| {
        set_style(doc, "#c2", "height: 9px; background: blue;")
    });
    oracle.step("z-index change deep in sc", |doc| {
        set_style(doc, "#z1", "position: relative; z-index: 5; height: 4px;")
    });
    oracle.step("z-index to zero", |doc| {
        set_style(doc, "#z2", "position: relative; z-index: 0; height: 4px;")
    });
    oracle.step("z-index back", |doc| {
        set_style(doc, "#z2", "position: relative; z-index: -2; height: 4px;")
    });
    oracle.step("sc stops being a stacking context root", |doc| {
        set_style(doc, "#sc", "height: 20px;")
    });
    oracle.step("sibling geometry change while sc is clean", |doc| {
        set_style(
            doc,
            "#abs",
            "position: absolute; top: 15px; left: 7px; width: 11px; height: 13px;",
        )
    });
    oracle.step("sc becomes a stacking context root again", |doc| {
        set_style(doc, "#sc", "height: 20px; transform: translateX(1px);")
    });
    oracle.step("static position drops hoisting", |doc| {
        set_style(doc, "#neg", "z-index: -1; height: 4px;")
    });
    oracle.step("flex item z-index removed", |doc| {
        set_style(doc, "#fz", "width:10px; height:10px;")
    });
    oracle.step("flex item z-index and order", |doc| {
        set_style(
            doc,
            "#fz",
            "width:10px; height:10px; z-index: 1; order: -5;",
        )
    });
    oracle.step("rel becomes a stacking context root", |doc| {
        set_style(doc, "#rel", "position: relative; height: 30px; z-index: 0;")
    });
    oracle.step("append hoisted child into clean subtree", |doc| {
        let parent = id(doc, "#deep");
        let mut m = doc.mutate();
        let child = m.create_element(qname("div"), Vec::new());
        m.set_attribute(
            child,
            attr("style"),
            "position: relative; z-index: 7; height: 2px;",
        );
        m.append_children(parent, &[child]);
    });
    oracle.step("remove hoisted child", |doc| {
        let node = id(doc, "#z1");
        doc.mutate().remove_node(node);
    });
}

/// Resolving in non-incremental mode must leave no damage behind (the clear
/// pass runs in both modes), including when nothing changed between resolves.
#[test]
fn non_incremental_resolve_clears_all_damage() {
    let mut doc = make_doc(FIXTURE, false);
    assert_no_damage(&doc, "initial");
    doc.resolve(0.0);
    assert_no_damage(&doc, "idle resolve");
    set_style(&mut doc, "#block", "width: 100px;");
    doc.resolve(0.0);
    assert_no_damage(&doc, "after mutation");
}
