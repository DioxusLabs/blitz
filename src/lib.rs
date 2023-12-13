use dioxus::prelude::*;
use euclid::{Rect, Scale, Size2D};
use servo_url::{ImmutableOrigin, ServoUrl};
use slab::Slab;
use std::collections::{HashMap, VecDeque};
use style::{
    animation::DocumentAnimationSet,
    context::{
        QuirksMode, RegisteredSpeculativePainters, SharedStyleContext, StyleContext,
        ThreadLocalStyleContext,
    },
    dom::{SendNode, TDocument, TElement, TNode, TShadowRoot},
    global_style_data::GLOBAL_STYLE_DATA,
    media_queries::MediaType,
    media_queries::{Device as StyleDevice, MediaList},
    selector_parser::SnapshotMap,
    servo_arc::Arc,
    shared_lock::{SharedRwLock, StylesheetGuards},
    stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet},
    stylist::Stylist,
    traversal::PerLevelTraversalData,
    traversal_flags::TraversalFlags,
};
use style_impls::{BlitzDocument, RealDom};

use crate::style_impls::{BlitzElement, BlitzTraversal};

mod style_impls;

static QUIRKS_MODE: QuirksMode = QuirksMode::NoQuirks;

fn make_document_stylesheet(css: &str) -> DocumentStyleSheet {
    DocumentStyleSheet(Arc::new(make_stylesheet(css)))
}

// todo: pass in the wgpu device specs
fn make_stylist() -> Stylist {
    let mut stylist = Stylist::new(
        StyleDevice::new(
            MediaType::screen(),
            QUIRKS_MODE,
            Size2D::new(800., 600.),
            Scale::new(1.0),
        ),
        QUIRKS_MODE,
    );

    stylist
}

fn make_stylesheet(css: &str) -> Stylesheet {
    let url_data = ServoUrl::from_url("data:text/css;charset=utf-8;base64,".parse().unwrap());
    let origin = Origin::UserAgent;
    let media_list = MediaList::empty();
    let shared_lock = SharedRwLock::new();
    let media = Arc::new(shared_lock.wrap(media_list));
    let stylesheet_loader = None;
    let allow_import_rules = AllowImportRules::Yes;

    style::stylesheets::Stylesheet::from_str(
        css,
        url_data,
        origin,
        media,
        shared_lock.clone(),
        stylesheet_loader,
        None,
        QUIRKS_MODE,
        0,
        allow_import_rules,
    )
}
fn handle_incremental() {

    // let restyles = std::mem::take(&mut data.pending_restyles);
    // debug!("Draining restyles: {}", restyles.len());

    // let mut map = SnapshotMap::new();
    // let elements_with_snapshot: Vec<_> = restyles
    //     .iter()
    //     .filter(|r| r.1.snapshot.is_some())
    //     .map(|r| unsafe {
    //         ServoLayoutNode::<LayoutData>::new(&r.0)
    //             .as_element()
    //             .unwrap()
    //     })
    //     .collect();

    // for (el, restyle) in restyles {
    //     let el: ServoLayoutElement<LayoutData> =
    //         unsafe { ServoLayoutNode::new(&el).as_element().unwrap() };

    //     // If we haven't styled this node yet, we don't need to track a
    //     // restyle.
    //     let mut style_data = match el.mutate_data() {
    //         Some(d) => d,
    //         None => {
    //             unsafe { el.unset_snapshot_flags() };
    //             continue;
    //         },
    //     };

    //     if let Some(s) = restyle.snapshot {
    //         unsafe { el.set_has_snapshot() };
    //         map.insert(el.as_node().opaque(), s);
    //     }

    //     // Stash the data on the element for processing by the style system.
    //     style_data.hint.insert(restyle.hint.into());
    //     style_data.damage = restyle.damage;
    //     debug!("Noting restyle for {:?}: {:?}", el, style_data);
    // }
}

fn build_layout_context() {
    // let mut layout_context = self.build_layout_context(
    //     guards.clone(),
    //     &map,
    //     origin,
    //     data.animation_timeline_value,
    //     &data.animations,
    //     data.stylesheets_changed,
    //     None,
    // );

    // let dirty_root = unsafe {
    //     ServoLayoutNode::<DOMLayoutData>::new(&data.dirty_root.unwrap())
    //         .as_element()
    //         .unwrap()
    // };

    // let traversal = RecalcStyle::new(layout_context);
    // let token = {
    //     let shared = DomTraversal::<ServoLayoutElement<DOMLayoutData>>::shared_context(&traversal);
    //     RecalcStyle::pre_traverse(dirty_root, shared)
    // };
}

fn build_style_context<'a, E: TElement>(
    stylist: &'a Stylist,
    guards: StylesheetGuards<'a>,
    snapshot_map: &'a SnapshotMap,
    origin: ImmutableOrigin,
    current_time_for_animations: f64,
    animations: &DocumentAnimationSet,
    stylesheets_changed: bool,
    registered_speculative_painters: &'a dyn RegisteredSpeculativePainters,
) -> SharedStyleContext<'a> {
    SharedStyleContext {
        traversal_flags: match stylesheets_changed {
            true => TraversalFlags::ForCSSRuleChanges,
            false => TraversalFlags::empty(),
        },
        stylist,
        options: GLOBAL_STYLE_DATA.options.clone(),
        guards,
        visited_styles_enabled: false,
        animations: animations.clone(),
        current_time_for_animations,
        snapshot_map,
        registered_speculative_painters,
    }
}

#[test]
fn render_simple() {
    // Figured out a single-pass system from the servo repo itself:
    //
    // components/layout_thread_2020/lib.rs:795
    //  handle_reflow
    // tests/unit/style/custom_properties.rs

    // Make some CSS
    let stylesheet = make_document_stylesheet(
        r#"
        body {
            background-color: red;
        }

        div {
            background-color: blue;
        }

        div:hover {
            background-color: green;
        }
        "#,
    );

    // Make the real domtree by converting dioxus vnodes
    let markup = RealDom::from_dioxus(rsx! {
        body {
            div { background_color: "red", padding: "10px",
                div { "hello world" }
            }
        }
    });

    // servo requires a bunch of locks to be manually held
    let (author, user) = (SharedRwLock::new(), SharedRwLock::new());

    let guards = StylesheetGuards {
        author: &author.read(),
        ua_or_user: &user.read(),
    };
    let guard_ = &GLOBAL_STYLE_DATA.shared_lock;

    let mut stylist = make_stylist();

    let guard = stylesheet.0.shared_lock.clone();

    // Add the stylesheets to the stylist
    stylist.append_stylesheet(stylesheet, &guard.read());

    // We don't really need to do this, but it's worth keeping it here anyways
    stylist.force_stylesheet_origins_dirty(Origin::Author.into());

    // Create a styling context for use throughout the following passes.
    // In servo we'd also create a layout context, but since servo isn't updated with the new layout code, we're just using the styling context
    // In a different world we'd use both
    let painters = style_impls::RegisteredPaintersImpl;
    let animations = DocumentAnimationSet::default();
    let snapshots = SnapshotMap::new();

    let shared = build_style_context::<BlitzElement>(
        &stylist,
        guards,
        &snapshots,
        ImmutableOrigin::new_opaque(),
        0.0,
        &animations,
        false,
        &painters,
    );

    // markup.root().as_node()
    // components/style/driver.rs
    let root = markup.root();

    let scoped_tls = None;
    let mut tls = ThreadLocalStyleContext::<BlitzElement>::new();
    let traversal = BlitzTraversal::new();
    let mut context = StyleContext {
        shared: &shared,
        thread_local: &mut tls,
    };

    let work_unit_max = 1;
    let mut discovered = VecDeque::with_capacity(work_unit_max * 2);
    discovered.push_back(unsafe { SendNode::new(root.as_node()) });
    let current_dom_depth = 1; // root.depth();
    style::parallel::style_trees(
        &mut context,
        discovered,
        root.as_node().opaque(),
        work_unit_max,
        (|| 32)(),
        PerLevelTraversalData { current_dom_depth },
        None,
        &traversal,
        scoped_tls.as_ref(),
    );

    // create a token
    // I don't think we need it?
    // let token = RecalcStyle::pre_traverse(dirty_root, shared);
    // let mut box_tree = Default::default();
    // if !BoxTree::update(traversal.context(), dirty_root) {
    //     *box_tree = Some(Arc::new(BoxTree::construct(traversal.context(), root_node)));
    // }

    // Now we need to do what handle_reflow does in servo
    // Reflows should be fast due to caching in the Stylist object
    //
    //
    // We can force reflows. Happens in the script/document section
    // The browser keeps track of pending restyles itself when attributes are changed.
    // When something like val.set_attribute() happens, the pending reflow is inserted into the list.
    // Once ticks are finished, the ScriptReflow object is created and sent to the layout thread.
    // The layout thread uses the ScriptReflow object to inform itself on what changes need to happen.
    // Zooming and touching causes full reflows.
    // For this demo we want to do complete reflows (have yet to figure it out)
    // But eventually we'll want to queue up modifications to the ECS engine and then build the script-reflow type object.
    // Unfortunately, this API assumes nodes are backed by pointers which adds some unsafe where we wouldn't want it.
    //
    // Reflow allows us to specify a dirty root node and a list of nodes to reflow.
    //
    // Notes:
    // - https://developers.google.com/speed/docs/insights/browser-reflow
    // - components/script/dom/window.rs:force_reflow

    // servo would use perform_post_style_recalc_layout_passes to update layouts before sending to webrenderer
    // for the sake of this demo, we construct a layout tree from taffy

    // now, render the nodes out using wgpu
}
