//! Regression test: a `position: fixed` element with `transform: translate(-50%, -50%)`
//! centered on screen must be hit-testable. A full-screen overlay (`mask-overlay`)
//! with lower z-index must not intercept clicks meant for the centered content.
//!
//! This mirrors the YearMonthModal layout: a `mask-overlay` (fixed, full-screen,
//! z-index via CSS var) and a `mask-content` (fixed, full-screen, higher z-index)
//! wrapping a `modal-content` (fixed, top:50%, left:50%, translate(-50%,-50%)).

use blitz_test_harness::{Harness, HarnessOptions};

fn harness(html: &str) -> Harness {
    Harness::from_html_with(
        html,
        HarnessOptions {
            width: 400,
            height: 400,
            ..Default::default()
        },
    )
}

#[test]
fn fixed_transformed_centered_content_is_hittable() {
    // 400×400 viewport. Content is 100×100, centered via top:50%/left:50% +
    // translate(-50%,-50%), so it paints at (150,150)-(250,250).
    // The overlay covers the full viewport at a lower z-index.
    let harness = harness(
        r#"<html><head><style>
            html, body { margin: 0; height: 100%; }
            .overlay {
                position: fixed;
                top: 0; left: 0;
                width: 100vw; height: 100vh;
                z-index: 10;
                background: rgba(0,0,0,0.2);
            }
            .content {
                position: fixed;
                top: 50%; left: 50%;
                transform: translate(-50%, -50%);
                z-index: 20;
                width: 100px;
                height: 100px;
                background: #fff;
            }
        </style></head><body>
            <div class="overlay" id="overlay"></div>
            <div class="content" id="content"></div>
        </body></html>"#,
    );

    let overlay = harness.node("#overlay");
    let content = harness.node("#content");

    // Click the center of the content (200, 200).
    let hit = harness.hit(200.0, 200.0);
    assert!(hit.is_some(), "expected a hit at center");
    let hit = hit.unwrap();
    assert_ne!(
        hit.node_id, overlay,
        "click at center should not hit the overlay"
    );
    assert_eq!(
        hit.node_id, content,
        "click at center should hit the transformed centered content"
    );

    // Click near the top-left corner of the content (155, 155).
    let hit = harness.hit(155.0, 155.0);
    assert!(hit.is_some(), "expected a hit at (155, 155)");
    let hit = hit.unwrap();
    assert_eq!(
        hit.node_id, content,
        "click at (155, 155) should hit the content, not the overlay"
    );
}

#[test]
fn fixed_transformed_centered_content_with_css_var_zindex() {
    // Same as above but z-index uses CSS custom properties + calc(),
    // matching the Mask component's pattern. Blitz's stylo engine
    // correctly resolves var() in z-index to integer values.
    let harness = harness(
        r#"<html><head><style>
            html, body { margin: 0; height: 100%; }
            :root { --mask-z: 1000; }
            .overlay {
                position: fixed;
                top: 0; left: 0;
                width: 100vw; height: 100vh;
                z-index: var(--mask-z);
                background: rgba(0,0,0,0.2);
            }
            .content {
                position: fixed;
                top: 50%; left: 50%;
                transform: translate(-50%, -50%);
                z-index: calc(var(--mask-z) + 1);
                width: 100px;
                height: 100px;
                background: #fff;
            }
        </style></head><body>
            <div class="overlay" id="overlay"></div>
            <div class="content" id="content"></div>
        </body></html>"#,
    );

    let overlay = harness.node("#overlay");
    let content = harness.node("#content");

    // Verify CSS var() resolves to integer z-index values
    {
        let doc = harness.base();
        if let Some(n) = doc.get_node(overlay) {
            assert_eq!(
                n.z_index(),
                1000,
                "overlay z_index should resolve var(--mask-z) to 1000"
            );
        }
        if let Some(n) = doc.get_node(content) {
            assert_eq!(
                n.z_index(),
                1001,
                "content z_index should resolve calc(var(--mask-z) + 1) to 1001"
            );
        }
    }

    let hit = harness.hit(200.0, 200.0);
    assert!(hit.is_some(), "expected a hit at center");
    let hit = hit.unwrap();
    assert_ne!(
        hit.node_id, overlay,
        "click at center should not hit the overlay"
    );
    assert_eq!(
        hit.node_id, content,
        "click at center should hit the content"
    );
}

#[test]
fn fixed_transformed_centered_content_nested_in_wrapper() {
    // Mirrors the actual structure: body > mask-overlay (sibling) +
    // mask-content (fixed, full-screen wrapper) > modal-content (fixed, centered).
    let harness = harness(
        r#"<html><head><style>
            .mask-overlay {
                position: fixed;
                top: 0; left: 0;
                width: 100vw; height: 100vh;
                z-index: 10;
                background: rgba(0,0,0,0.2);
            }
            .mask-content {
                position: fixed;
                top: 0; left: 0;
                width: 100vw; height: 100vh;
                z-index: 12;
            }
            .modal-content {
                position: fixed;
                top: 50%; left: 50%;
                transform: translate(-50%, -50%);
                z-index: 12;
                width: 100px;
                height: 100px;
                background: #fff;
            }
        </style></head><body style="margin:0">
            <div class="mask-overlay" id="overlay"></div>
            <div class="mask-content">
                <div class="modal-content" id="content"></div>
            </div>
        </body></html>"#,
    );

    let overlay = harness.node("#overlay");
    let content = harness.node("#content");

    let hit = harness.hit(200.0, 200.0);
    assert!(hit.is_some(), "expected a hit at center");
    let hit = hit.unwrap();
    assert_ne!(
        hit.node_id, overlay,
        "click at center should not hit the overlay"
    );
    assert_eq!(
        hit.node_id, content,
        "click at center should hit the nested centered content"
    );
}

#[test]
fn mask_content_does_not_intercept_clicks_on_nested_modal_content() {
    // Exact mirror of the real app:
    // - modal-content has NO explicit width/height (shrinks to content)
    // - modal-content contains a panel with its own width
    // - modal-content has overflow:auto, transform, top:50%/left:50%
    // - mask-content and modal-content both use CSS var z-index = calc(var(--mask-z)+1)
    let harness = Harness::from_html_with(
        r#"<html><head><style>
            html, body { margin: 0; }
            :root { --mask-z: 1000; }
            .app-root {
                height: calc(100vh - 2px);
                padding: 20px;
                overflow: auto;
                background: #f0f2f5;
            }
            .mask-overlay {
                position: fixed;
                top: 0; left: 0;
                width: 100vw; height: 100vh;
                z-index: var(--mask-z);
                background: rgba(0,0,0,0.2);
            }
            .mask-content {
                position: fixed;
                top: 0; left: 0;
                width: 100vw; height: 100vh;
                z-index: calc(var(--mask-z) + 1);
            }
            .modal-content {
                position: fixed;
                top: 50%; left: 50%;
                transform: translate(-50%, -50%);
                z-index: calc(var(--mask-z) + 1);
                background: #fff;
                border-radius: 12px;
                overflow: auto;
            }
            .panel {
                width: 220px;
                background: #fff;
            }
            .panel-columns {
                display: flex;
                height: 180px;
            }
            .panel-footer {
                display: flex;
                gap: 8px;
                padding: 10px 16px;
            }
            .panel-btn {
                flex: 1;
                height: 30px;
            }
        </style></head><body>
            <div class="app-root">
                <h1>Hisdata Export Tool</h1>
            </div>
            <div class="mask-overlay" id="overlay"></div>
            <div class="mask-content" id="mask-content">
                <div class="modal-content" id="content">
                    <div class="panel">
                        <div class="panel-columns">
                            <div style="flex:1; height:180px; overflow:hidden;">Col1</div>
                            <div style="flex:1; height:180px; overflow:hidden;">Col2</div>
                        </div>
                        <div class="panel-footer">
                            <div class="panel-btn" id="cancel-btn" style="background:#e5e7eb;">Cancel</div>
                            <div class="panel-btn" id="confirm-btn" style="background:#6366f1;">OK</div>
                        </div>
                    </div>
                </div>
            </div>
        </body></html>"#,
        HarnessOptions {
            width: 780,
            height: 600,
            ..Default::default()
        },
    );

    let overlay = harness.node("#overlay");
    let mask_content = harness.node("#mask-content");
    let content = harness.node("#content");
    let cancel_btn = harness.node("#cancel-btn");

    // Debug
    {
        let doc = harness.base();
        let mut cur = doc.get_node(content);
        while let Some(node) = cur {
            let l = node.final_layout();
            eprintln!(
                "node {:?}: loc=({},{}) size=({},{}) z={} sc={} transform={:?}",
                node.id,
                l.location.x,
                l.location.y,
                l.size.width,
                l.size.height,
                node.z_index(),
                node.stacking_context.is_some(),
                node.transform()
            );
            if let Some(sc) = &node.stacking_context {
                eprintln!("  sc content_area={:?}", sc.content_area);
                for hc in &sc.children {
                    eprintln!(
                        "  hoisted {:?}: z={} pos=({},{})",
                        hc.node_id, hc.z_index, hc.position.x, hc.position.y
                    );
                }
            }
            cur = node.parent.and_then(|p| doc.get_node(p));
        }
    }

    // Click cancel button. In a 780x600 viewport, modal-content is centered at (390, 300).
    // Panel is 220px wide. Footer is at the bottom. Cancel button is at left of footer.
    let hit = harness.hit(390.0, 300.0);
    assert!(hit.is_some(), "expected a hit");
    let hit = hit.unwrap();
    eprintln!("hit: {:?}", hit.node_id);
    assert_ne!(hit.node_id, overlay, "should not hit overlay");
    assert_ne!(hit.node_id, mask_content, "should not hit mask-content");
}

#[test]
fn mask_modal_structure_with_css_var_zindex_and_child_button() {
    // Exact mirror of the YearMonthModal structure:
    // - mask-overlay: fixed, full-screen, z=var(--mask-z)=1000
    // - mask-content: fixed, full-screen, z=calc(var(--mask-z)+1)=1002,
    //                 wrapping modal-content
    // - modal-content: fixed, top:50%/left:50%, translate(-50%,-50%),
    //                  z=calc(var(--mask-z)+1)=1002, contains a button
    // Clicking the button must hit the button, not mask-content or mask-overlay.
    let harness = harness(
        r#"<html><head><style>
            html, body { margin: 0; height: 100%; }
            :root { --mask-z: 1000; }
            .mask-overlay {
                position: fixed;
                top: 0; left: 0;
                width: 100vw; height: 100vh;
                z-index: var(--mask-z);
                background: rgba(0,0,0,0.2);
            }
            .mask-content {
                position: fixed;
                top: 0; left: 0;
                width: 100vw; height: 100vh;
                z-index: calc(var(--mask-z) + 1);
            }
            .modal-content {
                position: fixed;
                top: 50%; left: 50%;
                transform: translate(-50%, -50%);
                z-index: calc(var(--mask-z) + 1);
                width: 100px;
                height: 100px;
                background: #fff;
            }
        </style></head><body>
            <div class="mask-overlay" id="overlay"></div>
            <div class="mask-content" id="mask-content">
                <div class="modal-content" id="content">
                    <button id="btn" style="width:80px; height:30px;">Cancel</button>
                </div>
            </div>
        </body></html>"#,
    );

    let overlay = harness.node("#overlay");
    let mask_content = harness.node("#mask-content");
    let content = harness.node("#content");
    let btn = harness.node("#btn");

    // Click center of modal-content (200, 200) - should hit button or content
    let hit = harness.hit(200.0, 200.0);
    assert!(hit.is_some(), "expected a hit at center");
    let hit = hit.unwrap();
    assert_ne!(hit.node_id, overlay, "click should not hit mask-overlay");
    assert_ne!(
        hit.node_id, mask_content,
        "click should not hit mask-content, should reach content or button"
    );
    // Hit should be content or button (button is inside content)
    assert!(
        hit.node_id == content || hit.node_id == btn,
        "click should hit content or button, got {:?}",
        hit.node_id
    );
}

#[test]
fn dynamically_inserted_modal_content_is_hittable() {
    // Reproduces the real bug: modal-content is inserted AFTER initial render
    // (Vue v-if). The incremental layout path must correctly hoist modal-content
    // into mask-content's stacking context.
    use blitz_dom::{Attribute, DocumentMutator};
    use markup5ever::{QualName, ns};

    let mut harness = Harness::from_html_with(
        r#"<html><head><style>
            html, body { margin: 0; height: 100%; }
            :root { --mask-z: 1000; }
            .mask-overlay {
                position: fixed;
                top: 0; left: 0;
                width: 100vw; height: 100vh;
                z-index: var(--mask-z);
                background: rgba(0,0,0,0.2);
            }
            .mask-content {
                position: fixed;
                top: 0; left: 0;
                width: 100vw; height: 100vh;
                z-index: calc(var(--mask-z) + 1);
            }
            .modal-content {
                position: fixed;
                top: 50%; left: 50%;
                transform: translate(-50%, -50%);
                z-index: calc(var(--mask-z) + 1);
                background: #fff;
                overflow: auto;
            }
            .panel { width: 220px; }
            .panel-btn { width: 100px; height: 30px; }
        </style></head><body>
            <div class="app-root">
                <h1>App</h1>
            </div>
            <div class="mask-overlay" id="overlay"></div>
            <div class="mask-content" id="mask-content">
            </div>
        </body></html>"#,
        HarnessOptions {
            width: 780,
            height: 600,
            ..Default::default()
        },
    );

    // First pump: render with empty mask-content
    harness.pump();

    let mask_content = harness.node("#mask-content");

    // Now dynamically insert modal-content into mask-content (simulating Vue v-if)
    {
        let mut doc = harness.base_mut();
        let doc = &mut *doc;
        let mut m = DocumentMutator::new(doc);

        // Create modal-content
        let modal = m.create_element(
            QualName::new(None, ns!(html), "div".into()),
            vec![Attribute {
                name: QualName::new(None, ns!(), "class".into()),
                value: "modal-content".into(),
            }],
        );

        // Create panel inside modal-content
        let panel = m.create_element(
            QualName::new(None, ns!(html), "div".into()),
            vec![Attribute {
                name: QualName::new(None, ns!(), "class".into()),
                value: "panel".into(),
            }],
        );

        // Create cancel button
        let btn = m.create_element(
            QualName::new(None, ns!(html), "div".into()),
            vec![Attribute {
                name: QualName::new(None, ns!(), "class".into()),
                value: "panel-btn".into(),
            }],
        );

        let btn_text = m.create_text_node("Cancel");

        // Assemble: mask-content > modal-content > panel > btn > "Cancel"
        m.append_children(modal, &[panel]);
        m.append_children(panel, &[btn]);
        m.append_children(btn, &[btn_text]);
        m.append_children(mask_content, &[modal]);
    }

    // Second pump: resolve with the newly inserted modal-content
    harness.pump();

    // Click center of viewport (390, 300) - modal-content is centered there
    let hit = harness.hit(390.0, 300.0);
    assert!(hit.is_some(), "expected a hit at modal center");
    let hit = hit.unwrap();
    eprintln!("hit: {:?}", hit.node_id);
    assert_ne!(
        hit.node_id,
        harness.node("#overlay"),
        "should not hit overlay"
    );
    assert_ne!(hit.node_id, mask_content, "should not hit mask-content");
}

#[test]
fn dynamically_inserted_modal_content_hittable_at_transformed_edge() {
    // Edge case: modal-content has loc=(390,300) size=(220,30) with
    // transform: translate(-50%, -50%) = translate(-110, -15).
    // Rendered top-left = (280, 285). content_area (pre-transform) = (390,300,610,330).
    // Click at (285, 290) which is INSIDE the rendered modal but OUTSIDE content_area.
    use blitz_dom::{Attribute, DocumentMutator};
    use markup5ever::{QualName, ns};

    let mut harness = Harness::from_html_with(
        r#"<html><head><style>
            html, body { margin: 0; height: 100%; }
            :root { --mask-z: 1000; }
            .mask-overlay {
                position: fixed; top: 0; left: 0;
                width: 100vw; height: 100vh;
                z-index: var(--mask-z);
            }
            .mask-content {
                position: fixed; top: 0; left: 0;
                width: 100vw; height: 100vh;
                z-index: calc(var(--mask-z) + 1);
            }
            .modal-content {
                position: fixed; top: 50%; left: 50%;
                transform: translate(-50%, -50%);
                z-index: calc(var(--mask-z) + 1);
                background: #fff;
            }
            .panel { width: 220px; }
        </style></head><body>
            <div class="app-root"><h1>App</h1></div>
            <div class="mask-overlay" id="overlay"></div>
            <div class="mask-content" id="mask-content"></div>
        </body></html>"#,
        HarnessOptions {
            width: 780,
            height: 600,
            ..Default::default()
        },
    );

    harness.pump();
    let mask_content = harness.node("#mask-content");

    {
        let mut doc = harness.base_mut();
        let doc = &mut *doc;
        let mut m = DocumentMutator::new(doc);
        let modal = m.create_element(
            QualName::new(None, ns!(html), "div".into()),
            vec![Attribute {
                name: QualName::new(None, ns!(), "class".into()),
                value: "modal-content".into(),
            }],
        );
        let panel = m.create_element(
            QualName::new(None, ns!(html), "div".into()),
            vec![Attribute {
                name: QualName::new(None, ns!(), "class".into()),
                value: "panel".into(),
            }],
        );
        let text = m.create_text_node("Content");
        m.append_children(panel, &[text]);
        m.append_children(modal, &[panel]);
        m.append_children(mask_content, &[modal]);
    }

    harness.pump();

    // modal-content: loc=(390,300) size=(220,30), transform translate(-110,-15)
    // rendered: (280,285) to (500,315)
    // Click at (285, 290) - inside rendered modal, outside pre-transform content_area
    let hit = harness.hit(285.0, 290.0);
    assert!(hit.is_some(), "expected a hit at modal transformed edge");
    let hit = hit.unwrap();
    eprintln!("edge hit: {:?}", hit.node_id);
    assert_ne!(
        hit.node_id,
        harness.node("#overlay"),
        "should not hit overlay"
    );
    assert_ne!(hit.node_id, mask_content, "should not hit mask-content");
}
