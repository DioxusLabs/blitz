//! Regression tests for https://github.com/DioxusLabs/blitz/issues/624:
//! `paint_children` and stacking-context hoisted children are derived data
//! rebuilt during `resolve()`, so after `DocumentMutator::remove_and_drop_node`
//! (and before the next resolve) they can contain stale NodeIds. A hit test
//! run in that window must skip the stale ids rather than panic in
//! `Node::hit_inner` on `self.tree().get(id).unwrap()`.

use blitz_test_harness::{Harness, HarnessOptions};

#[test]
fn hit_test_after_remove_and_drop_node_does_not_panic() {
    let mut harness = Harness::from_html_with(
        r#"<html><head><style>
            html, body { margin: 0; height: 100%; }
            .container { width: 200px; height: 200px; position: relative; }
            .child { width: 100px; height: 100px; background: #ccc; }
        </style></head><body>
            <div class="container" id="container">
                <div class="child" id="child-a">A</div>
                <div class="child" id="child-b">B</div>
            </div>
        </body></html>"#,
        HarnessOptions {
            width: 400,
            height: 400,
            ..Default::default()
        },
    );

    // First pump: layout + paint_children are built.
    harness.pump();

    let child_a = harness.node("#child-a");

    // Remove child-a from the DOM without pumping (paint_children is stale).
    harness.base_mut().mutate().remove_and_drop_node(child_a);

    // Hit test in the area where child-a used to be. This must not panic even
    // though paint_children still contains child-a's id.
    let hit = harness.hit(50.0, 50.0);
    assert!(
        hit.is_some(),
        "hit test should succeed without panicking after node removal"
    );
    assert_ne!(hit.unwrap().node_id, child_a);
}

#[test]
fn hit_test_after_removing_hoisted_child_does_not_panic() {
    // A child with z-index is hoisted into its parent stacking context's
    // pos_z_hoisted_children list. Removing such a child and then hit-testing
    // must not panic on the stale hoisted child id.
    let mut harness = Harness::from_html_with(
        r#"<html><head><style>
            html, body { margin: 0; height: 100%; }
            .container { width: 200px; height: 200px; position: relative; }
            .child { width: 100px; height: 100px; position: absolute; }
            #child-a { top: 0; left: 0; background: #ccc; z-index: 10; }
            #child-b { top: 0; left: 100px; background: #ddd; z-index: 10; }
        </style></head><body>
            <div class="container" id="container">
                <div class="child" id="child-a">A</div>
                <div class="child" id="child-b">B</div>
            </div>
        </body></html>"#,
        HarnessOptions {
            width: 400,
            height: 400,
            ..Default::default()
        },
    );

    harness.pump();

    let child_a = harness.node("#child-a");
    harness.base_mut().mutate().remove_and_drop_node(child_a);

    // Hit test where child-a used to be. The hoisted children list in the
    // container's stacking context may still reference child-a's id.
    let hit = harness.hit(50.0, 50.0);
    assert!(
        hit.is_some(),
        "hit test should succeed without panicking after hoisted child removal"
    );
    assert_ne!(hit.unwrap().node_id, child_a);
}

#[test]
fn hit_test_after_removing_neg_z_hoisted_child_does_not_panic() {
    // A child with negative z-index is hoisted into the neg_z_hoisted_children
    // list of its parent's stacking context. Removing such a child and then
    // hit-testing must not panic on the stale hoisted child id.
    let mut harness = Harness::from_html_with(
        r#"<html><head><style>
            html, body { margin: 0; height: 100%; }
            .container { width: 200px; height: 200px; position: relative; z-index: 0; }
            .child { width: 100px; height: 100px; position: absolute; }
            #child-a { top: 0; left: 0; background: #ccc; z-index: -1; }
            #child-b { top: 0; left: 100px; background: #ddd; z-index: 1; }
        </style></head><body>
            <div class="container" id="container">
                <div class="child" id="child-a">A</div>
                <div class="child" id="child-b">B</div>
            </div>
        </body></html>"#,
        HarnessOptions {
            width: 400,
            height: 400,
            ..Default::default()
        },
    );

    harness.pump();

    let child_a = harness.node("#child-a");
    harness.base_mut().mutate().remove_and_drop_node(child_a);

    // Hit test where child-a used to be. The neg_z_hoisted_children list
    // in the container's stacking context may still reference child-a's id.
    let hit = harness.hit(50.0, 50.0);
    assert!(
        hit.is_some(),
        "hit test should succeed without panicking after neg-z hoisted child removal"
    );
    assert_ne!(hit.unwrap().node_id, child_a);
}

#[test]
fn hit_test_after_detaching_node_does_not_hit_detached_node() {
    // `remove_node` detaches a node without dropping it, so its id remains
    // live in the tree. The derived paint lists must be eagerly cleaned so a
    // hit test does not return the detached node at its stale position.
    let mut harness = Harness::from_html_with(
        r#"<html><head><style>
            html, body { margin: 0; height: 100%; }
            .container { width: 200px; height: 200px; position: relative; }
            .child { width: 100px; height: 100px; background: #ccc; }
        </style></head><body>
            <div class="container" id="container">
                <div class="child" id="child-a">A</div>
            </div>
        </body></html>"#,
        HarnessOptions {
            width: 400,
            height: 400,
            ..Default::default()
        },
    );

    harness.pump();

    let child_a = harness.node("#child-a");
    let container = harness.node("#container");
    harness.base_mut().mutate().remove_node(child_a);

    let hit = harness.hit(50.0, 50.0).expect("hit test should succeed");
    assert_ne!(
        hit.node_id, child_a,
        "hit test must not return a node that has been detached from the DOM"
    );
    assert_eq!(hit.node_id, container);
}

#[test]
fn hit_test_after_detaching_hoisted_node_does_not_hit_detached_node() {
    // Same as above, but for a z-indexed child hoisted into its parent
    // stacking context's hoisted children list.
    let mut harness = Harness::from_html_with(
        r#"<html><head><style>
            html, body { margin: 0; height: 100%; }
            .container { width: 200px; height: 200px; position: relative; }
            #child-a { width: 100px; height: 100px; position: absolute;
                       top: 0; left: 0; background: #ccc; z-index: 10; }
        </style></head><body>
            <div class="container" id="container">
                <div id="child-a">A</div>
            </div>
        </body></html>"#,
        HarnessOptions {
            width: 400,
            height: 400,
            ..Default::default()
        },
    );

    harness.pump();

    let child_a = harness.node("#child-a");
    let container = harness.node("#container");
    harness.base_mut().mutate().remove_node(child_a);

    let hit = harness.hit(50.0, 50.0).expect("hit test should succeed");
    assert_ne!(
        hit.node_id, child_a,
        "hit test must not return a node that has been detached from the DOM"
    );
    assert_eq!(hit.node_id, container);
}

#[test]
fn hit_test_after_detaching_display_contents_node_does_not_hit_descendants() {
    // A `display: contents` node is transparent for box generation, so its
    // children's layout parent (and paint_children entries) live *outside*
    // the removed subtree. Detaching the contents node must eagerly clean up
    // entries for its descendants, not just the removed root itself.
    let mut harness = Harness::from_html_with(
        r#"<html><head><style>
            html, body { margin: 0; height: 100%; }
            .container { width: 200px; height: 200px; position: relative; }
            #contents { display: contents; }
            #child-a { width: 100px; height: 100px; background: #ccc; }
        </style></head><body>
            <div class="container" id="container">
                <div id="contents">
                    <div id="child-a">A</div>
                </div>
            </div>
        </body></html>"#,
        HarnessOptions {
            width: 400,
            height: 400,
            ..Default::default()
        },
    );

    harness.pump();

    let contents = harness.node("#contents");
    let child_a = harness.node("#child-a");
    let container = harness.node("#container");
    harness.base_mut().mutate().remove_node(contents);

    let hit = harness.hit(50.0, 50.0).expect("hit test should succeed");
    assert_ne!(
        hit.node_id, child_a,
        "hit test must not return a descendant of a detached display:contents node"
    );
    assert_eq!(hit.node_id, container);
}

#[test]
fn hit_test_after_removing_text_node_does_not_panic() {
    // The inline layout (built during resolve) references text node ids. After
    // removing a text node's parent, hit-testing over the old text area must
    // not panic or return the stale text node id.
    let mut harness = Harness::from_html_with(
        r#"<html><head><style>
            html, body { margin: 0; height: 100%; }
            #container { width: 200px; height: 200px; font-size: 20px; }
        </style></head><body>
            <div id="container"><span id="text-span">Hello world</span></div>
        </body></html>"#,
        HarnessOptions {
            width: 400,
            height: 400,
            ..Default::default()
        },
    );

    harness.pump();

    let span = harness.node("#text-span");
    harness.base_mut().mutate().remove_and_drop_node(span);

    // Hit test over where the text used to be.
    let hit = harness.hit(10.0, 10.0);
    assert!(
        hit.is_some(),
        "hit test should succeed without panicking after text node removal"
    );
    assert_ne!(hit.unwrap().node_id, span);
}
