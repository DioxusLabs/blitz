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
//!
//! Every box is additionally compared structurally (inline root / table root
//! flags, list markers), so a missing re-collect is caught even when the
//! rects happen to coincide, and after every step both documents are checked
//! for dangling node ids and `layout_parent` consistency.

use blitz_dom::net::{Resource, ResourceLoadResponse};
use blitz_dom::node::NodeFlags;
use blitz_dom::util::ImageType;
use blitz_dom::{DocumentConfig, LocalName, QualName, ns};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::node_id::NodeId;
use blitz_traits::shell::{ColorScheme, Viewport};
use std::collections::HashSet;
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
    assert_eq!(
        structure_of(inc, inc_id),
        structure_of(non, non_id),
        "[{step}] box structure mismatch at {path} ({})",
        describe(inc, inc_id)
    );
}

/// Construction results of a node which are not reflected in its rect:
/// (is_anonymous, is_inline_root, is_table_root, list marker).
fn structure_of(doc: &HtmlDocument, node_id: NodeId) -> (bool, bool, bool, Option<String>) {
    let node = doc.get_node(node_id).unwrap();
    let marker = node
        .element_data()
        .and_then(|el| el.list_item_data.as_ref())
        .map(|item| format!("{:?}", item.marker));
    (
        node.is_anonymous(),
        node.flags.is_inline_root(),
        node.flags.is_table_root(),
        marker,
    )
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
    fn walk(doc: &HtmlDocument, node_id: NodeId, step: &str, visited: &mut HashSet<NodeId>) {
        if !visited.insert(node_id) {
            return;
        }
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
            walk(doc, *child, step, visited);
        }
        if let Some(children) = node.layout_children.borrow().as_ref() {
            for child in children.iter() {
                walk(doc, *child, step, visited);
            }
        }
        if let Some(before) = node.before() {
            walk(doc, before, step, visited);
        }
        if let Some(after) = node.after() {
            walk(doc, after, step, visited);
        }
    }
    walk(doc, doc.root_node().id, step, &mut HashSet::new());
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
        assert_consistent(&self.inc, step);
        assert_consistent(&self.non, step);
    }

    /// Apply the same mutation to both documents, resolve, and compare.
    /// Returns the number of nodes the incremental document reconstructed.
    fn step(&mut self, name: &str, mutation: impl Fn(&mut HtmlDocument)) -> usize {
        for doc in [&mut self.inc, &mut self.non] {
            mutation(doc);
            doc.resolve(0.0);
        }
        self.check(name);
        self.inc.reconstructed_node_count()
    }

    /// Hover the centre of `selector` (a pure restyle), then compare.
    fn hover(&mut self, selector: &str) -> usize {
        let (x, y) = center(&self.inc, selector);
        self.step(&format!("hover {selector}"), |doc| {
            assert!(doc.set_hover_to(x, y), "hover {selector}");
        })
    }

    fn unhover(&mut self) -> usize {
        self.step("unhover", |doc| {
            doc.set_hover_to(-10.0, -10.0);
        })
    }
}

/// Every node id referenced from an in-document node must be live in the
/// slab, and every layout child must point back at its container.
fn assert_consistent(doc: &HtmlDocument, step: &str) {
    doc.assert_layout_parents_consistent();
    let tree = doc.tree();
    for (node_id, node) in tree.iter() {
        if !node.flags.contains(NodeFlags::IS_IN_DOCUMENT) {
            continue;
        }
        let check = |kind: &str, ids: &mut dyn Iterator<Item = NodeId>| {
            for id in ids {
                assert!(
                    tree.get(id).is_some(),
                    "[{step}] dangling {kind} {id:?} on {}",
                    describe(doc, node_id)
                );
            }
        };
        check(
            "layout child",
            &mut node.layout_children.borrow().iter().flatten().copied(),
        );
        check(
            "paint child",
            &mut node.paint_children.borrow().iter().flatten().copied(),
        );
        check(
            "hoisted child",
            &mut node.hoisted_children.borrow().iter().copied(),
        );
        check(
            "anonymous block",
            &mut node.anonymous_blocks.iter().copied(),
        );
        check(
            "stacking context child",
            &mut node
                .stacking_context
                .iter()
                .flat_map(|sc| sc.children.iter().map(|child| child.node_id)),
        );
        check(
            "stacking context contribution",
            &mut node
                .sc_contribution_cache
                .borrow()
                .iter()
                .map(|child| child.node_id),
        );
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
    let reconstructed = oracle.step("hover on", |doc| {
        assert!(doc.set_hover_to(x, y));
    });
    assert_eq!(reconstructed, 0, "relayout-only hover");
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

    let reconstructed = oracle.step("hover off", |doc| {
        assert!(doc.set_hover_to(390.0, 390.0));
    });
    assert_eq!(reconstructed, 0, "relayout-only unhover");
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

    // Setting the `style` attribute puts construction damage on the node, so
    // the node and its collector (`<body>`) reconstruct, and nothing else.
    let reconstructed = oracle.step("width", |doc| set_style(doc, "#block", "width: 200px;"));
    assert_eq!(reconstructed, 2, "width");

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
        let reconstructed = oracle.step(&format!("set_node_text {text:?}"), |doc| {
            let span = id(doc, "#span");
            let text_node = doc.get_node(span).unwrap().children[0];
            doc.mutate().set_node_text(text_node, text);
        });
        // The inline root (and the two anonymous blocks it re-creates), and
        // nothing above it.
        assert_eq!(reconstructed, 3, "set_node_text");
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
        let reconstructed = oracle.step(&format!("hover {selector}"), |doc| {
            assert!(doc.set_hover_to(x, y));
        });
        assert_eq!(reconstructed, 0, "hover {selector}");
        let target = id(&oracle.inc, selector);
        let mut hovered = oracle.inc.get_hover_node_id();
        while let Some(node_id) = hovered
            && node_id != target
        {
            hovered = oracle.inc.get_node(node_id).unwrap().parent;
        }
        assert_eq!(hovered, Some(target), "hover {selector}");
        let reconstructed = oracle.step(&format!("unhover {selector}"), |doc| {
            assert!(doc.set_hover_to(390.0, 390.0));
        });
        assert_eq!(reconstructed, 0, "unhover {selector}");
    }

    let reconstructed = oracle.step("colour only", |doc| {
        set_style(doc, "#c2", "height: 9px; background: blue;")
    });
    // `#c2` and its collector (`#block`, through the `display:contents` parent).
    assert_eq!(reconstructed, 2, "colour only");
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

const BASE_STYLE: &str =
    "body { margin: 0; font-size: 10px; line-height: 1; font-family: sans-serif; width: 300px; }";

fn page(style: &str, body: &str) -> String {
    format!("<html><head><style>{BASE_STYLE}{style}</style></head><body>{body}</body></html>")
}

fn element(
    doc: &mut HtmlDocument,
    tag: &str,
    attrs: &[(&str, &str)],
    text: Option<&str>,
) -> NodeId {
    let mut m = doc.mutate();
    let node = m.create_element(qname(tag), Vec::new());
    for (name, value) in attrs {
        m.set_attribute(node, attr(name), value);
    }
    if let Some(text) = text {
        let text_node = m.create_text_node(text);
        m.append_children(node, &[text_node]);
    }
    node
}

fn first_text_child(doc: &HtmlDocument, selector: &str) -> NodeId {
    let node = doc.get_node(id(doc, selector)).unwrap();
    node.children
        .iter()
        .copied()
        .find(|child| doc.get_node(*child).unwrap().text_data().is_some())
        .unwrap_or_else(|| panic!("{selector} has no text child"))
}

fn remove(doc: &mut HtmlDocument, selector: &str) {
    let node = id(doc, selector);
    doc.mutate().remove_node(node);
}

fn set_attr(doc: &mut HtmlDocument, selector: &str, name: &str, value: &str) {
    let node = id(doc, selector);
    doc.mutate().set_attribute(node, attr(name), value);
}

#[test]
fn text_edit_under_nested_spans() {
    let mut oracle = Oracle::new(&page(
        "",
        r#"<p id="p"><b><i id="i">t</i></b> tail</p><div style="height:4px"></div>"#,
    ));
    for text in [
        "a much longer text under nested spans which wraps across several lines",
        "",
        "short",
    ] {
        let reconstructed = oracle.step(&format!("set text {text:?}"), |doc| {
            let text_node = first_text_child(doc, "#i");
            doc.mutate().set_node_text(text_node, text);
        });
        assert_eq!(reconstructed, 1, "only the inline root re-collects");
    }
}

/// Construction damage stops at the nearest collector: mutations deep in a
/// nested tree reconstruct the mutated node's container and nothing above it.
#[test]
fn reconstruction_is_local() {
    let depth = 30;
    let mut body = String::new();
    for level in 0..depth {
        body.push_str(&format!(r#"<div id="l{level}" style="padding-left:1px">"#));
    }
    body.push_str(r#"<p id="deep">deep <b><i id="di">text</i></b> tail</p><div id="leafhost" style="height:4px"></div>"#);
    for _ in 0..depth {
        body.push_str("</div>");
    }
    body.push_str(
        r#"<div id="flex" style="display:flex"><div id="fa">a</div><div class="x" id="fb">b</div></div>"#,
    );
    let mut oracle = Oracle::new(&page(
        "#flex:hover .x { order: -1; } #l5:hover { color: red; }",
        &body,
    ));

    let reconstructed = oracle.step("deep text edit", |doc| {
        let text_node = first_text_child(doc, "#di");
        doc.mutate()
            .set_node_text(text_node, "a much longer text which wraps");
    });
    assert_eq!(reconstructed, 1, "deep text edit: the inline root only");

    let reconstructed = oracle.step("block inserted at depth 10", |doc| {
        let blk = element(doc, "div", &[("id", "blk"), ("style", "height:5px")], None);
        let parent = id(doc, "#l10");
        doc.mutate().append_children(parent, &[blk]);
    });
    assert_eq!(
        reconstructed, 2,
        "block insertion: the new block and its container"
    );

    let reconstructed = oracle.step("block removed at depth 10", |doc| remove(doc, "#blk"));
    assert_eq!(reconstructed, 1, "block removal: the container only");

    let reconstructed = oracle.step("block inserted into deep paragraph", |doc| {
        let blk = element(doc, "div", &[("id", "pblk"), ("style", "height:5px")], None);
        let parent = id(doc, "#deep");
        doc.mutate().append_children(parent, &[blk]);
    });
    // The paragraph, its new child and the anonymous block wrapping the
    // paragraph's inline content.
    assert_eq!(reconstructed, 3, "block insertion into a paragraph");

    assert_eq!(oracle.hover("#l5"), 0, "colour-only hover");
    assert_eq!(oracle.unhover(), 0, "colour-only unhover");

    assert_eq!(
        oracle.hover("#flex"),
        1,
        "order change: the flex container only"
    );
    assert_eq!(
        layout_child_ids(&oracle.inc, "#flex")[0],
        Some("fb".to_string())
    );
    assert_eq!(oracle.unhover(), 1);
}

/// `display` toggled through every box-generating role on a block-level
/// element and on an inline one, each holding both inline and block content.
#[test]
fn display_toggles_on_block_and_span() {
    let mut oracle = Oracle::new(&page(
        "",
        r#"<div id="host">lead <div id="d">d text <b>bold</b><div style="height:3px"></div> tail</div> mid
           <span id="s">s text <i>it</i> <div style="height:2px"></div> end</span> trail</div>"#,
    ));
    let displays = [
        "inline",
        "block",
        "contents",
        "none",
        "table",
        "inline-block",
        "table-row",
        "flex",
        "table-cell",
        "contents",
        "table",
        "none",
        "inline",
        "grid",
        "block",
    ];
    for selector in ["#d", "#s"] {
        for display in displays {
            oracle.step(&format!("{selector} display:{display}"), |doc| {
                set_style(doc, selector, &format!("display: {display}"))
            });
        }
    }
}

#[test]
fn block_inserted_into_inline_span() {
    let mut oracle = Oracle::new(&page(
        "",
        r#"<div id="host"><span id="s">a <em id="em">b</em> c</span> after</div>"#,
    ));
    oracle.step("insert block into span", |doc| {
        let blk = element(
            doc,
            "div",
            &[("id", "blk"), ("style", "height:5px")],
            Some("blk"),
        );
        let anchor = id(doc, "#em");
        doc.mutate().insert_nodes_before(anchor, &[blk]);
    });
    oracle.step("append second block to span", |doc| {
        let blk = element(doc, "div", &[("id", "blk2")], Some("tail block"));
        let parent = id(doc, "#s");
        doc.mutate().append_children(parent, &[blk]);
    });
    oracle.step("remove one of two blocks", |doc| remove(doc, "#blk"));
}

/// A node which stops generating a box of its own (becomes `display:contents`,
/// or an inline element which no longer contains a block and folds back into
/// its parent's inline formatting context) is never visited by
/// `resolve_layout_children` again; its `layout_children` / `paint_children`
/// must be torn down rather than keep listing boxes which now belong to
/// another container or have been removed from the document.
#[test]
fn block_removed_from_inline_span() {
    let mut oracle = Oracle::new(&page(
        "",
        r#"<div id="host"><span id="s">a <em id="em">b</em> c</span> after</div>"#,
    ));
    oracle.step("insert block into span", |doc| {
        let blk = element(
            doc,
            "div",
            &[("id", "blk"), ("style", "height:5px")],
            Some("blk"),
        );
        let anchor = id(doc, "#em");
        doc.mutate().insert_nodes_before(anchor, &[blk]);
    });
    oracle.step("remove block from span", |doc| remove(doc, "#blk"));
    oracle.step("append block to span", |doc| {
        let blk = element(doc, "div", &[("id", "blk2")], Some("tail block"));
        let parent = id(doc, "#s");
        doc.mutate().append_children(parent, &[blk]);
    });
    oracle.step("remove appended block", |doc| remove(doc, "#blk2"));
}

#[test]
fn display_contents_children_mutated() {
    let mut oracle = Oracle::new(&page(
        "",
        r#"<div id="outer">
            <div id="dc" style="display: contents"><span>x</span> text <div id="dcb" style="height:5px"></div></div>
            <div id="after" style="height:3px"></div>
        </div>"#,
    ));
    oracle.step("append into contents", |doc| {
        let child = element(doc, "div", &[("id", "dcn"), ("style", "height:4px")], None);
        let parent = id(doc, "#dc");
        doc.mutate().append_children(parent, &[child]);
    });
    oracle.step("remove from contents", |doc| remove(doc, "#dcb"));
    oracle.step("text into contents", |doc| {
        let parent = id(doc, "#dc");
        let mut m = doc.mutate();
        let text = m.create_text_node(" more text ");
        m.append_children(parent, &[text]);
    });
    oracle.step("contents to block", |doc| {
        set_style(doc, "#dc", "display: block")
    });
}

/// A node which stops generating a box of its own (becomes `display:contents`,
/// or an inline element which no longer contains a block and folds back into
/// its parent's inline formatting context) is never visited by
/// `resolve_layout_children` again; its `layout_children` / `paint_children`
/// must be torn down rather than keep listing boxes which now belong to
/// another container or have been removed from the document.
#[test]
fn display_contents_toggle_and_children() {
    let mut oracle = Oracle::new(&page(
        "#outer:hover #dc2 { display: contents; }",
        r#"<div id="outer">
            <div id="dc"><span>x</span> text <div id="dcb" style="height:5px"></div></div>
            <div id="dc2" style="padding: 3px"><div style="height:2px"></div> inner</div>
            <div id="after" style="height:3px"></div>
        </div>"#,
    ));
    oracle.step("dc contents", |doc| {
        set_style(doc, "#dc", "display: contents")
    });
    oracle.step("append into contents", |doc| {
        let child = element(doc, "div", &[("id", "dcn"), ("style", "height:4px")], None);
        let parent = id(doc, "#dc");
        doc.mutate().append_children(parent, &[child]);
    });
    oracle.step("remove from contents", |doc| remove(doc, "#dcb"));
    oracle.step("text into contents", |doc| {
        let parent = id(doc, "#dc");
        let mut m = doc.mutate();
        let text = m.create_text_node(" more text ");
        m.append_children(parent, &[text]);
    });
    oracle.step("dc block", |doc| set_style(doc, "#dc", "display: block"));
    oracle.step("dc contents again", |doc| {
        set_style(doc, "#dc", "display: contents")
    });
    oracle.step("dc none", |doc| set_style(doc, "#dc", "display: none"));
    oracle.step("dc contents from none", |doc| {
        set_style(doc, "#dc", "display: contents")
    });
    oracle.hover("#after");
    oracle.unhover();
}

#[test]
fn table_mutations() {
    let mut oracle = Oracle::new(&page(
        "td { height: 10px; }",
        r#"<table id="t"><tbody id="tb">
            <tr id="r1"><td id="c1">a</td><td id="c2">b</td></tr>
            <tr id="r2"><td id="c3">c</td><td>d</td></tr>
        </tbody></table>"#,
    ));
    oracle.step("insert tr", |doc| {
        let tr = element(doc, "tr", &[("id", "r3")], None);
        let td1 = element(doc, "td", &[], Some("new cell"));
        let td2 = element(doc, "td", &[], Some("x"));
        doc.mutate().append_children(tr, &[td1, td2]);
        let anchor = id(doc, "#r2");
        doc.mutate().insert_nodes_before(anchor, &[tr]);
    });
    oracle.step("remove tr", |doc| remove(doc, "#r1"));
    oracle.step("append td", |doc| {
        let td = element(doc, "td", &[("id", "c4")], Some("third column"));
        let parent = id(doc, "#r2");
        doc.mutate().append_children(parent, &[td]);
    });
    oracle.step("remove td", |doc| remove(doc, "#c3"));
    oracle.step("colspan 2", |doc| set_attr(doc, "#c4", "colspan", "2"));
    oracle.step("colspan 1", |doc| set_attr(doc, "#c4", "colspan", "1"));
    oracle.step("append tr to tbody", |doc| {
        let tr = element(doc, "tr", &[], None);
        let td = element(doc, "td", &[("colspan", "3")], Some("wide"));
        doc.mutate().append_children(tr, &[td]);
        let parent = id(doc, "#tb");
        doc.mutate().append_children(parent, &[tr]);
    });
    oracle.step("new tbody", |doc| {
        let tbody = element(doc, "tbody", &[("id", "tb2")], None);
        let tr = element(doc, "tr", &[], None);
        let td = element(doc, "td", &[], Some("second body"));
        doc.mutate().append_children(tr, &[td]);
        doc.mutate().append_children(tbody, &[tr]);
        let parent = id(doc, "#t");
        doc.mutate().append_children(parent, &[tbody]);
    });
    oracle.step("tbody inner html", |doc| {
        let node = id(doc, "#tb");
        doc.mutate()
            .set_inner_html(node, "<tr><td>p</td><td>q</td><td>r</td></tr>");
    });
}

#[test]
fn pseudo_elements_appear_disappear_change() {
    let mut oracle = Oracle::new(&page(
        r#".pb::before { content: "B"; }
           .pa::after { content: "AA"; display: block; height: 3px; }
           .pc::before { content: "CCC"; }
           .hvb::before { content: "x"; }
           .hvb:hover::before { content: "hovered content"; }
           #hvn:hover::after { content: "new"; display: block; }"#,
        r#"<p id="pp">x <span id="ps">span</span> y</p>
           <div id="pd">block</div>
           <div id="hv" class="hvb" style="height: 20px">hover me</div>
           <div id="hvn" style="height: 20px">hover me too</div>"#,
    ));
    for class in ["pb", "pc", ""] {
        oracle.step(&format!("span class {class:?}"), |doc| {
            set_attr(doc, "#ps", "class", class)
        });
    }
    oracle.step("span class pb again", |doc| {
        set_attr(doc, "#ps", "class", "pb")
    });
    // Re-collecting the paragraph flushes the span's existing pseudo without
    // damaging it: only the paragraph reconstructs, and no damage is left
    // behind on the pseudo (checked by `assert_no_damage`).
    let reconstructed = oracle.step("text edit next to span with pseudo", |doc| {
        let text_node = first_text_child(doc, "#pp");
        doc.mutate().set_node_text(text_node, "edited ");
    });
    assert_eq!(
        reconstructed, 1,
        "text edit in a paragraph with a pseudo-bearing span"
    );
    for class in ["pb", "pb pa", "pc", "pc pa", ""] {
        oracle.step(&format!("block class {class:?}"), |doc| {
            set_attr(doc, "#pd", "class", class)
        });
    }
    oracle.hover("#hv");
    oracle.unhover();
    oracle.hover("#hvn");
    oracle.unhover();
}

#[test]
fn bare_text_in_flex_and_grid() {
    let mut oracle = Oracle::new(&page(
        "",
        r#"<div id="fx" style="display:flex"><div id="fx1" style="width:10px;height:10px"></div></div>
           <div id="gd" style="display:grid; grid-template-columns: 30px 30px"><div id="g1" style="height:10px"></div></div>"#,
    ));
    for container in ["#fx", "#gd"] {
        oracle.step(&format!("append text to {container}"), |doc| {
            let parent = id(doc, container);
            let mut m = doc.mutate();
            let text = m.create_text_node("hello");
            m.append_children(parent, &[text]);
        });
        oracle.step(&format!("prepend text to {container}"), |doc| {
            let anchor = doc.get_node(id(doc, container)).unwrap().children[0];
            let mut m = doc.mutate();
            let text = m.create_text_node("first");
            m.insert_nodes_before(anchor, &[text]);
        });
        for _ in 0..2 {
            oracle.step(&format!("remove text from {container}"), |doc| {
                let text = first_text_child(doc, container);
                doc.mutate().remove_node(text);
            });
        }
    }
}

fn layout_child_ids(doc: &HtmlDocument, selector: &str) -> Vec<Option<String>> {
    doc.get_node(id(doc, selector))
        .unwrap()
        .layout_children
        .borrow()
        .iter()
        .flatten()
        .map(|child| {
            doc.get_node(*child)
                .unwrap()
                .element_data()
                .and_then(|el| el.id.as_ref().map(|id| id.to_string()))
        })
        .collect()
}

/// `order` changes (pure restyles) on flex items next to an anonymous block,
/// and inside an inline-flex container which is itself wrapped in an
/// anonymous block: the anonymous block carries no damage, but the container
/// below it must still re-sort.
#[test]
fn order_change_next_to_and_inside_anonymous_blocks() {
    let mut oracle = Oracle::new(&page(
        "#of:hover .x { order: -1; } #wrap:hover .y { order: -1; }",
        r#"<div id="of" style="display:flex"><span id="oa">a</span>text<span class="x" id="ox">b</span></div>
           <div id="wrap">lead <div id="if" style="display:inline-flex"><span id="i1">a</span><span class="y" id="i2">b</span></div> trail<div style="height:4px"></div></div>"#,
    ));
    let initial = layout_child_ids(&oracle.inc, "#of");
    assert_eq!(initial.first(), Some(&Some("oa".to_string())));
    // The flex container re-sorts, re-creating the anonymous block around its
    // bare text; nothing above it is touched.
    assert_eq!(
        oracle.hover("#of"),
        2,
        "flex container and its anonymous block"
    );
    for doc in [&oracle.inc, &oracle.non] {
        assert_eq!(
            layout_child_ids(doc, "#of").first(),
            Some(&Some("ox".to_string())),
            "incremental={}",
            doc.incremental_layout()
        );
    }
    oracle.unhover();
    assert_eq!(layout_child_ids(&oracle.inc, "#of"), initial);

    assert_eq!(
        oracle.hover("#wrap"),
        1,
        "only the inline-flex container re-sorts"
    );
    for doc in [&oracle.inc, &oracle.non] {
        assert_eq!(
            layout_child_ids(doc, "#if"),
            vec![Some("i2".to_string()), Some("i1".to_string())],
            "incremental={}",
            doc.incremental_layout()
        );
    }
    oracle.unhover();
}

#[test]
fn move_nodes_between_span_and_div() {
    let mut oracle = Oracle::new(&page(
        "",
        r#"<p id="mp">para <span id="msp">in <b id="mb">bold</b> span</span> end</p>
           <div id="mdiv"><div id="mblk" style="height:6px"></div> text</div>"#,
    ));
    oracle.step("span child to div", |doc| {
        let (node, parent) = (id(doc, "#mb"), id(doc, "#mdiv"));
        doc.mutate().append_children(parent, &[node]);
    });
    oracle.step("div child to span", |doc| {
        let (node, parent) = (id(doc, "#mblk"), id(doc, "#msp"));
        doc.mutate().append_children(parent, &[node]);
    });
    oracle.step("back to span start", |doc| {
        let node = id(doc, "#mb");
        let anchor = doc.get_node(id(doc, "#msp")).unwrap().children[0];
        doc.mutate().insert_nodes_before(anchor, &[node]);
    });
}

/// A node which stops generating a box of its own (becomes `display:contents`,
/// or an inline element which no longer contains a block and folds back into
/// its parent's inline formatting context) is never visited by
/// `resolve_layout_children` again; its `layout_children` / `paint_children`
/// must be torn down rather than keep listing boxes which now belong to
/// another container or have been removed from the document.
#[test]
fn move_block_out_of_span() {
    let mut oracle = Oracle::new(&page(
        "",
        r#"<p id="mp">para <span id="msp">in <b id="mb">bold</b> span</span> end</p>
           <div id="mdiv"><div id="mblk" style="height:6px"></div> text</div>"#,
    ));
    oracle.step("span child to div", |doc| {
        let (node, parent) = (id(doc, "#mb"), id(doc, "#mdiv"));
        doc.mutate().append_children(parent, &[node]);
    });
    oracle.step("div child to span", |doc| {
        let (node, parent) = (id(doc, "#mblk"), id(doc, "#msp"));
        doc.mutate().append_children(parent, &[node]);
    });
    oracle.step("back to span start", |doc| {
        let node = id(doc, "#mb");
        let anchor = doc.get_node(id(doc, "#msp")).unwrap().children[0];
        doc.mutate().insert_nodes_before(anchor, &[node]);
    });
    oracle.step("block back to div", |doc| {
        let node = id(doc, "#mblk");
        let anchor = doc.get_node(id(doc, "#mdiv")).unwrap().children[0];
        doc.mutate().insert_nodes_before(anchor, &[node]);
    });
}

#[test]
fn form_controls() {
    let mut oracle = Oracle::new(&page(
        "",
        r#"<div id="fc">label <input id="in" type="text" value="v"> after</div>"#,
    ));
    for ty in ["checkbox", "radio", "text", "hidden", "text"] {
        oracle.step(&format!("input type={ty}"), |doc| {
            set_attr(doc, "#in", "type", ty)
        });
    }
    oracle.step("insert textarea", |doc| {
        let ta = element(doc, "textarea", &[("id", "ta")], Some("textarea text"));
        let parent = id(doc, "#fc");
        doc.mutate().append_children(parent, &[ta]);
    });
    oracle.step("remove textarea", |doc| remove(doc, "#ta"));
}

#[test]
fn br_and_whitespace_only_anonymous_blocks() {
    let mut oracle = Oracle::new(&page(
        "",
        r#"<div id="bw"><div style="height:5px"></div> <span id="bws">x</span> <div style="height:3px"></div></div>
           <p id="brp">line one<br id="br1">line two</p>"#,
    ));
    oracle.step("remove only inline content", |doc| remove(doc, "#bws"));
    oracle.step("re-add inline content", |doc| {
        let span = element(doc, "span", &[("id", "bws")], Some("back"));
        let parent = id(doc, "#bw");
        doc.mutate().append_children(parent, &[span]);
    });
    oracle.step("append br", |doc| {
        let br = element(doc, "br", &[("id", "br2")], None);
        let parent = id(doc, "#brp");
        doc.mutate().append_children(parent, &[br]);
    });
    oracle.step("remove br", |doc| remove(doc, "#br1"));
    oracle.step("br into block container", |doc| {
        let br = element(doc, "br", &[], None);
        let parent = id(doc, "#bw");
        doc.mutate().append_children(parent, &[br]);
    });
}

#[test]
fn list_items_and_markers() {
    let mut oracle = Oracle::new(&page(
        "",
        r#"<ol id="ol"><li>a</li><li id="li2">b</li></ol>
           <ol id="ol2" start="5"><div id="olc" style="display:contents"><li>x</li><ol id="nested"><li>n1</li></ol></div><li>y</li></ol>"#,
    ));
    oracle.step("insert li", |doc| {
        let li = element(doc, "li", &[], Some("inserted"));
        let anchor = id(doc, "#li2");
        doc.mutate().insert_nodes_before(anchor, &[li]);
    });
    oracle.step("append li", |doc| {
        let li = element(doc, "li", &[], Some("appended"));
        let parent = id(doc, "#ol");
        doc.mutate().append_children(parent, &[li]);
    });
    oracle.step("remove li", |doc| remove(doc, "#li2"));
    oracle.step("append nested li", |doc| {
        let li = element(doc, "li", &[], Some("n2"));
        let parent = id(doc, "#nested");
        doc.mutate().append_children(parent, &[li]);
    });
    oracle.step("li into contents", |doc| {
        let li = element(doc, "li", &[], Some("in contents"));
        let parent = id(doc, "#olc");
        doc.mutate().append_children(parent, &[li]);
    });
    oracle.step("reversed", |doc| set_attr(doc, "#ol", "reversed", ""));
    oracle.step("start", |doc| set_attr(doc, "#ol2", "start", "10"));
}

#[test]
fn image_load() {
    const URL: &str = "https://example.com/a.png";
    let mut oracle = Oracle::new(&page(
        "",
        &format!(
            r#"<div id="imh">text <img id="img" src="{URL}"> more text</div>
               <div><img id="img2" src="{URL}" style="display:block"></div>"#
        ),
    ));
    let reconstructed = oracle.step("image loaded", |doc| {
        doc.load_resource(ResourceLoadResponse {
            request_id: usize::MAX,
            node_id: None,
            resolved_url: Some(URL.to_string()),
            result: Ok(Resource::Image(
                ImageType::Image,
                20,
                10,
                Arc::new(vec![255; 20 * 10 * 4]),
            )),
        });
    });
    assert_eq!(reconstructed, 0, "an image load only changes a leaf's size");
    for selector in ["#img", "#img2"] {
        assert_eq!(
            layout_of(&oracle.inc, id(&oracle.inc, selector)).unwrap().2,
            20.0,
            "{selector}"
        );
    }
}

/// A table root's Taffy style is baked into its `TableContext` at
/// construction, so a restyle of the table (`width`, `border-spacing`,
/// inherited `border-spacing` changing when the table is moved out of an
/// ancestor table) must reconstruct it, not just lay it out again.
#[test]
fn table_root_restyle() {
    let mut oracle = Oracle::new(&page(
        "#tw:hover { width: 200px; } #ts:hover { border-spacing: 10px; } #tc:hover { border-collapse: collapse; } #pad:hover td { padding: 0; }",
        r#"<table id="tw"><tr><td>a</td><td>b</td></tr></table>
           <table id="ts"><tr><td>a</td><td>b</td></tr></table>
           <table id="tc"><tr><td style="border: 1px solid">a</td><td style="border: 1px solid">b</td></tr></table>
           <div id="pad"><table><tr><td>c</td></tr></table></div>
           <table id="outer"><tr><td><div id="g" style="display:grid; grid-template-columns: 20px"><div style="display:table">text</div></div></td></tr></table>"#,
    ));
    // The table and its collector (`<body>`).
    for selector in ["#tw", "#ts", "#tc"] {
        assert_eq!(oracle.hover(selector), 2, "hover {selector}");
        oracle.unhover();
    }
    assert_eq!(
        oracle.hover("#pad"),
        0,
        "cell padding is read at layout time"
    );
    oracle.unhover();
    let reconstructed = oracle.step("grid moved out of the outer table", |doc| {
        let grid = id(doc, "#g");
        let body = id(doc, "body");
        doc.mutate().append_children(body, &[grid]);
    });
    // The inner table's inherited `border-spacing` changed: it reconstructs
    // along with its collector (the grid), the `<td>` it left and `<body>`.
    assert_eq!(reconstructed, 4);
}

/// A cell placed directly in a row group (no `<tr>`) is a layout child of
/// the table like any other cell; its content changes must reach it.
#[test]
fn cell_directly_in_row_group() {
    // The parser would foster-parent a `<p>` out of the `<tbody>`.
    let mut oracle = Oracle::new(&page(
        "",
        r#"<table><tbody id="tb"><tr><td>a</td></tr></tbody></table>"#,
    ));
    oracle.step("cell inserted into the row group", |doc| {
        let cell = element(
            doc,
            "p",
            &[("id", "c"), ("style", "display:table-cell")],
            Some("c2"),
        );
        let tbody = id(doc, "#tb");
        doc.mutate().append_children(tbody, &[cell]);
    });
    oracle.step("text edit in the cell", |doc| {
        let text_node = first_text_child(doc, "#c");
        doc.mutate().set_node_text(text_node, "a longer cell text");
    });
    oracle.step("cell becomes whitespace-only", |doc| {
        let text_node = first_text_child(doc, "#c");
        doc.mutate().set_node_text(text_node, " ");
    });
    oracle.step("cell gets a block", |doc| {
        let blk = element(doc, "div", &[("style", "height:5px")], None);
        let cell = id(doc, "#c");
        doc.mutate().append_children(cell, &[blk]);
    });
}

/// Hover/focus changes affecting only paint reconstruct no boxes.
#[test]
fn paint_only_state_changes_reconstruct_nothing() {
    let mut oracle = Oracle::new(&page(
        "#cc:hover { background: blue; } #fo:focus { background: green; } #rl:hover { height: 30px; }",
        r#"<div id="cc" style="width:50px;height:20px">c</div>
           <div id="fo" tabindex="0" style="height:10px">f</div>
           <div id="rl" style="height:20px">relayout only</div>"#,
    ));
    assert_eq!(oracle.hover("#cc"), 0, "colour-only hover");
    assert_eq!(oracle.unhover(), 0, "colour-only unhover");
    assert_eq!(
        oracle.step("focus", |doc| {
            let node = id(doc, "#fo");
            assert!(doc.set_focus_to(node));
        }),
        0,
        "focus"
    );
    assert_eq!(oracle.hover("#rl"), 0, "relayout-only hover");
    assert_eq!(oracle.unhover(), 0, "relayout-only unhover");
}

#[test]
fn element_styled_for_the_first_time() {
    let mut oracle = Oracle::new(&page(
        "#hid2 { display: none; } #host2:hover #hid2 { display: block; }",
        r#"<div id="hid" style="display:none"><p>hidden <span>text</span></p><div style="height:5px"></div></div>
           <div id="host2" style="min-height: 20px">host <div id="hid2"><p>also <b>hidden</b></p></div></div>"#,
    ));
    oracle.step("show", |doc| set_style(doc, "#hid", "display: block"));
    oracle.step("hide", |doc| set_style(doc, "#hid", "display: none"));
    oracle.step("show again", |doc| set_style(doc, "#hid", "display: block"));
    oracle.hover("#host2");
    oracle.unhover();
}

/// Properties which are baked into the Parley layout at construction time
/// must reconstruct the inline formatting context when they change.
#[test]
fn inline_layout_baked_properties_via_hover() {
    let mut oracle = Oracle::new(&page(
        "#lh:hover #lhs { line-height: 3; }
         #ti:hover { text-indent: 40px; }
         #nw:hover { text-wrap-mode: nowrap; }
         #nws:hover #nwss { text-wrap-mode: nowrap; }
         p { width: 100px; }",
        r#"<p id="lh">some text <span id="lhs">span text that wraps a bit</span> tail</p>
           <p id="ti">indented paragraph text which wraps</p>
           <p id="nw">no wrap text which would otherwise wrap</p>
           <p id="nws">text <span id="nwss">no wrap span text which would wrap</span> end</p>"#,
    ));
    for selector in ["#lh", "#ti", "#nw", "#nws"] {
        oracle.hover(selector);
        oracle.unhover();
    }
}

/// A block pseudo-element on a span is an inline box nested in an inline
/// element: its `layout_parent` is the inline root, not the span.
#[test]
fn block_pseudo_element_on_span() {
    let mut oracle = Oracle::new(&page(
        r#".pa::after { content: "AA"; display: block; height: 3px; }"#,
        r#"<p id="pp">x <span id="ps">span</span> y</p>"#,
    ));
    oracle.step("block ::after on span", |doc| {
        set_attr(doc, "#ps", "class", "pa")
    });
    oracle.step("remove block ::after", |doc| {
        set_attr(doc, "#ps", "class", "")
    });
}

/// An inline-block nested in an inline element is a layout child of the
/// inline root, and its `layout_parent` stays the inline root across frames.
#[test]
fn inline_block_nested_in_inline_element() {
    let mut oracle = Oracle::new(&page(
        "",
        r#"<p id="p">t <a id="a"><span id="ib" style="display:inline-block; width:10px; height:10px"></span></a> u</p>
           <div id="x" style="height:3px"></div>"#,
    ));
    oracle.step("unrelated change", |doc| {
        set_style(doc, "#x", "height: 4px")
    });
    oracle.step("resize nested inline-block", |doc| {
        set_style(doc, "#ib", "display:inline-block; width:20px; height:10px")
    });
}

/// Teardown of nodes which stop generating a box (see
/// `block_removed_from_inline_span`), here for the descendants of a node
/// which becomes `display:none`: the removed node must not still be listed
/// by its (now box-less) former container.
#[test]
fn remove_from_display_none_subtree() {
    let mut oracle = Oracle::new(&page(
        "",
        r#"<div id="outer"><div><div id="d">z</div></div></div>"#,
    ));
    oracle.step("hide", |doc| set_style(doc, "#outer", "display: none"));
    oracle.step("remove from hidden subtree", |doc| remove(doc, "#d"));
}

/// Teardown of nodes which stop generating a box (see
/// `block_removed_from_inline_span`), here for a `display:none` child skipped
/// by its mixed inline/block container: its old construction flags must be
/// reset, and moving it (which does not damage the moved node itself) into a
/// container which does list it must not expose them.
#[test]
fn move_display_none_node() {
    let mut oracle = Oracle::new(&page(
        "",
        r#"<div id="host"><div id="mixed">block text<div id="inner">inner</div></div></div>"#,
    ));
    oracle.step("hide inner", |doc| {
        set_style(doc, "#inner", "display: none")
    });
    oracle.step("move inner into block container", |doc| {
        let (node, anchor) = (id(doc, "#inner"), id(doc, "#mixed"));
        doc.mutate().insert_nodes_before(anchor, &[node]);
    });
}

/// Table-internal elements are constructed by their table root
/// (`build_table_context`), not as boxes of their own. When the table changes
/// `display` only the table itself is damaged, yet its row groups / rows start
/// (or stop) generating boxes and must be constructed (or torn down).
#[test]
fn table_display_change() {
    let mut oracle = Oracle::new(&page(
        "",
        r#"<table id="t"><tbody><tr><td>a</td><td>b</td></tr></tbody></table>
           <div id="d"><div style="display:table-row"><div style="display:table-cell">c</div></div></div>"#,
    ));
    oracle.step("table to block", |doc| {
        set_style(doc, "#t", "display: block")
    });
    oracle.step("block to table", |doc| {
        set_style(doc, "#d", "display: table")
    });
}

/// An inline-block wrapped in an anonymous block becomes `display:contents`:
/// the anonymous block is freed when its container re-collects, so the
/// inline-block's box state must be torn down even though no surviving
/// container ever listed it.
#[test]
fn inline_block_in_anonymous_block_becomes_contents() {
    let mut oracle = Oracle::new(&page(
        "",
        r#"<span><p></p>text<span id="t" style="display:inline-block">it<div style="display:table-row-group">text</div></span></span>"#,
    ));
    oracle.step("to contents", |doc| {
        set_style(doc, "#t", "display: contents")
    });
    oracle.step("back to inline-block", |doc| {
        set_style(doc, "#t", "display: inline-block")
    });
}

/// Known bug (found by `fuzz_restricted`, seed index 67, corpus 3; not a
/// construction issue): when an ancestor becomes a flex container, a nested
/// paragraph is measured under new constraints (which re-break its Parley
/// layout) but its final layout hits the Taffy cache from the previous frame,
/// so the lines stay broken at the measurement width. Clearing the inline
/// roots' Taffy caches before layout makes the scenario pass.
#[test]
#[ignore = "known bug: inline root final-layout cache hit after a measure pass re-broke its lines"]
fn nested_flex_ancestor_becomes_flex() {
    let mut oracle = Oracle::new(&page(
        "",
        r#"<div id="o"><div style="display:flex"><div><p>para <span style="display:inline-block; width:20px; height:5px"></span> tail</p></div></div></div>"#,
    ));
    oracle.step("outer to flex", |doc| set_style(doc, "#o", "display: flex"));
}

// ---------------------------------------------------------------------------
// Seeded random mutation fuzzer
// ---------------------------------------------------------------------------

/// xorshift64: deterministic across platforms and runs.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> Option<T> {
        (!items.is_empty()).then(|| items[self.below(items.len())])
    }
}

const FUZZ_STYLE: &str = r#"
    .o { order: -1; }
    .fs { font-size: 14px; }
    .lh { line-height: 2; }
    .pos { position: absolute; top: 3px; left: 5px; }
    .bef::before { content: "B"; }
    .aft::after { content: "after"; }
"#;

const FUZZ_CORPUS: [&str; 4] = [
    r#"<div><p>Hello <b>bold <i>it</i></b> world</p><div>block text<div>inner</div></div>
       <section><p>second <span>span</span> tail</p></section></div>"#,
    r#"<div style="display:flex"><div>a</div><div class="aft">b</div>text<div>c</div></div>
       <div style="display:grid; grid-template-columns: 20px 20px"><div>1</div><div>2</div><div>3</div></div>"#,
    r#"<table><tbody><tr><td>a</td><td>b <span>c</span></td></tr><tr><td>d</td><td>e</td></tr></tbody></table>
       <div style="display:contents"><div>c1</div><p>c2</p></div><ol><li>one</li><li>two</li></ol>"#,
    r#"<div class="bef"><p class="bef">para <span style="display:inline-block; width:20px; height:5px"></span> tail</p></div>
       <div style="position:relative; height:30px"><div class="pos">abs</div><div>rel child</div></div>"#,
];

const WORDS: [&str; 8] = [
    "lorem",
    "ipsum",
    "a",
    "wrapping",
    "text",
    "x",
    "longerword",
    " ",
];

/// Block-level tags: in restricted mode only these are inserted, moved,
/// positioned or have their `display` toggled, so that no inline element
/// ever gains or loses a box of its own.
const BLOCK_TAGS: [&str; 5] = ["div", "p", "section", "li", "ol"];
/// Tags which may receive inserted/moved children.
const CONTAINER_TAGS: [&str; 5] = ["body", "div", "section", "li", "td"];
/// Table-internal containers (unrestricted mode only): rows are inserted
/// into these, cells into rows.
const TABLE_CONTAINER_TAGS: [&str; 3] = ["table", "tbody", "tr"];

/// Child-index path from `<body>`.
type Path = Vec<usize>;

#[derive(Debug, Clone)]
enum FuzzOp {
    SetText(Path, String),
    Insert {
        parent: Path,
        before: Option<usize>,
        tag: &'static str,
        text: String,
    },
    Remove(Path),
    Move {
        node: Path,
        parent: Path,
        before: Option<usize>,
    },
    Display(Path, &'static str),
    Class(Path, &'static str),
    Colspan(Path, &'static str),
}

fn body_id(doc: &HtmlDocument) -> NodeId {
    id(doc, "body")
}

fn node_at(doc: &HtmlDocument, path: &[usize]) -> NodeId {
    path.iter().fold(body_id(doc), |node, index| {
        doc.get_node(node).unwrap().children[*index]
    })
}

fn tag_of(doc: &HtmlDocument, node_id: NodeId) -> Option<String> {
    doc.get_node(node_id)
        .unwrap()
        .element_data()
        .map(|el| el.name.local.to_string())
}

/// All element and text nodes below `<body>` (with their paths), skipping
/// the contents of `<textarea>` (whose text is only read at construction).
fn fuzz_nodes(doc: &HtmlDocument) -> Vec<(Path, NodeId)> {
    fn walk(doc: &HtmlDocument, node_id: NodeId, path: &mut Path, out: &mut Vec<(Path, NodeId)>) {
        for (i, child) in doc.get_node(node_id).unwrap().children.iter().enumerate() {
            let node = doc.get_node(*child).unwrap();
            if node.element_data().is_none() && node.text_data().is_none() {
                continue;
            }
            path.push(i);
            out.push((path.clone(), *child));
            if tag_of(doc, *child).as_deref() != Some("textarea") {
                walk(doc, *child, path, out);
            }
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(doc, body_id(doc), &mut Vec::new(), &mut out);
    out
}

fn random_text(rng: &mut Rng) -> String {
    (0..rng.below(6))
        .map(|_| rng.pick(&WORDS).unwrap())
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_descendant_or_self(doc: &HtmlDocument, node: NodeId, ancestor: NodeId) -> bool {
    let mut current = Some(node);
    while let Some(id) = current {
        if id == ancestor {
            return true;
        }
        current = doc.get_node(id).unwrap().parent;
    }
    false
}

/// Choose a random applicable operation against the current tree. In
/// restricted mode structural changes are limited to block-level elements.
fn choose_op(doc: &HtmlDocument, rng: &mut Rng, restricted: bool) -> Option<FuzzOp> {
    let nodes = fuzz_nodes(doc);
    let elements: Vec<(Path, NodeId, String)> = nodes
        .iter()
        .filter_map(|(path, node)| tag_of(doc, *node).map(|tag| (path.clone(), *node, tag)))
        .collect();
    let is_block = |tag: &str| BLOCK_TAGS.contains(&tag);
    let blocks: Vec<&(Path, NodeId, String)> = elements
        .iter()
        .filter(|(_, _, tag)| is_block(tag))
        .collect();
    let mut containers: Vec<(Path, NodeId)> = elements
        .iter()
        .filter(|(_, _, tag)| {
            CONTAINER_TAGS.contains(&tag.as_str())
                || (!restricted
                    && (["span", "b", "i", "p"].contains(&tag.as_str())
                        || TABLE_CONTAINER_TAGS.contains(&tag.as_str())))
        })
        .map(|(path, node, _)| (path.clone(), *node))
        .collect();
    containers.push((Vec::new(), body_id(doc)));
    let random_before = |rng: &mut Rng, parent: NodeId, exclude: Option<NodeId>| {
        let children = &doc.get_node(parent).unwrap().children;
        let candidates: Vec<usize> = (0..children.len())
            .filter(|i| Some(children[*i]) != exclude)
            .collect();
        match rng.below(3) {
            0 => None,
            _ => rng.pick(&candidates),
        }
    };

    match rng.below(if restricted { 6 } else { 7 }) {
        0 => {
            let texts: Vec<&Path> = nodes
                .iter()
                .filter(|(_, node)| doc.get_node(*node).unwrap().text_data().is_some())
                .map(|(path, _)| path)
                .collect();
            let path = texts.get(rng.below(texts.len().max(1)))?;
            Some(FuzzOp::SetText((*path).clone(), random_text(rng)))
        }
        1 => {
            let (parent, parent_id) = containers[rng.below(containers.len())].clone();
            let parent_tag = tag_of(doc, parent_id).unwrap_or_default();
            let tags: &[&'static str] = if restricted {
                &["div", "p"]
            } else if parent_tag == "tr" {
                &["td"]
            } else if parent_tag == "table" || parent_tag == "tbody" {
                &["tr", "tbody"]
            } else {
                &["div", "span", "b", "p", "li", "table"]
            };
            Some(FuzzOp::Insert {
                parent,
                before: random_before(rng, parent_id, None),
                tag: rng.pick(tags).unwrap(),
                text: random_text(rng),
            })
        }
        2 => {
            let candidates: Vec<&Path> = if restricted {
                blocks.iter().map(|(path, _, _)| path).collect()
            } else {
                elements.iter().map(|(path, _, _)| path).collect()
            };
            let path = candidates.get(rng.below(candidates.len().max(1)))?;
            Some(FuzzOp::Remove((*path).clone()))
        }
        3 => {
            let candidates: Vec<(&Path, NodeId)> = if restricted {
                blocks.iter().map(|(path, node, _)| (path, *node)).collect()
            } else {
                elements
                    .iter()
                    .map(|(path, node, _)| (path, *node))
                    .collect()
            };
            let (node, node_id) = *candidates.get(rng.below(candidates.len().max(1)))?;
            let parents: Vec<&(Path, NodeId)> = containers
                .iter()
                .filter(|(_, parent)| !is_descendant_or_self(doc, *parent, node_id))
                .collect();
            let (parent, parent_id) = (*parents.get(rng.below(parents.len().max(1)))?).clone();
            Some(FuzzOp::Move {
                node: node.clone(),
                before: random_before(rng, parent_id, Some(node_id)),
                parent,
            })
        }
        4 => {
            let (path, node_id, tag) = if restricted {
                (*blocks.get(rng.below(blocks.len().max(1)))?).clone()
            } else {
                elements.get(rng.below(elements.len().max(1)))?.clone()
            };
            let values: &[&'static str] = if !restricted {
                &[
                    "inline",
                    "block",
                    "contents",
                    "none",
                    "flex",
                    "grid",
                    "inline-block",
                    "table",
                    "table-row",
                    "table-cell",
                ]
            } else if tag == "div"
                && doc
                    .get_node(node_id)
                    .unwrap()
                    .children
                    .iter()
                    .all(|child| tag_of(doc, *child).is_none_or(|tag| is_block(&tag)))
            {
                &["block", "flex", "grid", "flow-root"]
            } else {
                &["block", "flow-root"]
            };
            Some(FuzzOp::Display(path, rng.pick(values).unwrap()))
        }
        5 => {
            let (path, _, tag) = elements.get(rng.below(elements.len().max(1)))?.clone();
            let classes: &[&'static str] = if restricted && !is_block(&tag) {
                &["", "o", "fs", "lh", "o lh"]
            } else {
                &["", "o", "fs", "lh", "pos", "o lh", "fs pos"]
            };
            Some(FuzzOp::Class(path, rng.pick(classes).unwrap()))
        }
        _ => {
            let cells: Vec<&Path> = elements
                .iter()
                .filter(|(_, _, tag)| tag == "td")
                .map(|(path, _, _)| path)
                .collect();
            let path = cells.get(rng.below(cells.len().max(1)))?;
            Some(FuzzOp::Colspan(
                (*path).clone(),
                rng.pick(&["1", "2", "3"]).unwrap(),
            ))
        }
    }
}

fn apply_op(doc: &mut HtmlDocument, op: &FuzzOp) {
    match op {
        FuzzOp::SetText(path, text) => {
            let node = node_at(doc, path);
            doc.mutate().set_node_text(node, text);
        }
        FuzzOp::Insert {
            parent,
            before,
            tag,
            text,
        } => {
            let parent = node_at(doc, parent);
            let node = element(doc, tag, &[], Some(text));
            match before {
                Some(index) => {
                    let anchor = doc.get_node(parent).unwrap().children[*index];
                    doc.mutate().insert_nodes_before(anchor, &[node]);
                }
                None => doc.mutate().append_children(parent, &[node]),
            }
        }
        FuzzOp::Remove(path) => {
            let node = node_at(doc, path);
            doc.mutate().remove_node(node);
        }
        FuzzOp::Move {
            node,
            parent,
            before,
        } => {
            let node = node_at(doc, node);
            let parent = node_at(doc, parent);
            match before {
                Some(index) => {
                    let anchor = doc.get_node(parent).unwrap().children[*index];
                    doc.mutate().insert_nodes_before(anchor, &[node]);
                }
                None => doc.mutate().append_children(parent, &[node]),
            }
        }
        FuzzOp::Display(path, display) => {
            let node = node_at(doc, path);
            doc.mutate()
                .set_attribute(node, attr("style"), &format!("display: {display}"));
        }
        FuzzOp::Class(path, class) => {
            let node = node_at(doc, path);
            doc.mutate().set_attribute(node, attr("class"), class);
        }
        FuzzOp::Colspan(path, colspan) => {
            let node = node_at(doc, path);
            doc.mutate().set_attribute(node, attr("colspan"), colspan);
        }
    }
}

fn run_fuzzer(seeds: &[u64], steps: usize, restricted: bool) {
    for (corpus_index, body) in FUZZ_CORPUS.iter().enumerate() {
        for &seed in seeds {
            let seed_index = seed / 0x9e37_79b9;
            let seed = seed ^ ((corpus_index as u64 + 1) << 32);
            let mut rng = Rng(seed);
            let mut log: Vec<FuzzOp> = Vec::new();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut oracle = Oracle::new(&page(FUZZ_STYLE, body));
                for step in 0..steps {
                    let Some(op) = choose_op(&oracle.inc, &mut rng, restricted) else {
                        continue;
                    };
                    log.push(op.clone());
                    oracle.step(&format!("fuzz step {step}: {op:?}"), |doc| {
                        apply_op(doc, &op)
                    });
                }
            }));
            if let Err(panic) = result {
                eprintln!(
                    "fuzzer failed: corpus {corpus_index}, seed {seed:#x} (FUZZ_SEED={seed_index}), restricted={restricted}"
                );
                for (i, op) in log.iter().enumerate() {
                    eprintln!("  {i}: {op:?}");
                }
                std::panic::resume_unwind(panic);
            }
        }
    }
}

/// Seeds and step count from the environment: `FUZZ_SEEDS=n` runs seed
/// indices `1..=n`, `FUZZ_SEED=i` a single index (as reported by a failure),
/// `FUZZ_STEPS` the number of mutations per seed.
fn fuzz_config(default_seeds: &[u64], default_steps: usize) -> (Vec<u64>, usize) {
    let seed = |i: u64| i * 0x9e37_79b9;
    let seeds = if let Ok(i) = std::env::var("FUZZ_SEED") {
        vec![seed(i.parse().unwrap())]
    } else if let Ok(n) = std::env::var("FUZZ_SEEDS") {
        (1..=n.parse::<u64>().unwrap()).map(seed).collect()
    } else {
        default_seeds.to_vec()
    };
    let steps = std::env::var("FUZZ_STEPS").map_or(default_steps, |n| n.parse().unwrap());
    (seeds, steps)
}

/// Random mutations restricted to block-level structural changes.
#[test]
fn fuzz_restricted() {
    let (seeds, steps) = fuzz_config(&[0x9e37_79b9, 0x2545_f491], 30);
    run_fuzzer(&seeds, steps, true);
}

/// Unrestricted random mutations (inline elements gaining/losing boxes,
/// `display:contents`, nested inline boxes).
#[test]
fn fuzz_unrestricted() {
    let (seeds, steps) = fuzz_config(&[0x9e37_79b9, 0x2545_f491], 30);
    run_fuzzer(&seeds, steps, false);
}
