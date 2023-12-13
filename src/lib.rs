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
    dom::{NodeInfo, SendNode, TDocument, TElement, TNode, TShadowRoot},
    driver::traverse_dom,
    global_style_data::GLOBAL_STYLE_DATA,
    media_queries::MediaType,
    media_queries::{Device as StyleDevice, MediaList},
    selector_parser::SnapshotMap,
    servo_arc::Arc,
    shared_lock::{SharedRwLock, StylesheetGuards},
    stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet},
    stylist::Stylist,
    thread_state::ThreadState,
    traversal::{resolve_style, DomTraversal, PerLevelTraversalData, PreTraverseToken},
    traversal_flags::TraversalFlags,
};
use style_impls::{BlitzNode, RealDom};
use style_traverser::RecalcStyle;

mod style_impls;
mod style_traverser;

static QUIRKS_MODE: QuirksMode = QuirksMode::NoQuirks;

fn make_document_stylesheet(css: &str) -> DocumentStyleSheet {
    DocumentStyleSheet(Arc::new(make_stylesheet(css)))
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

pub fn render(css: &str, markup: LazyNodes) {
    // Figured out a single-pass system from the servo repo itself:
    //
    // components/layout_thread_2020/lib.rs:795
    //  handle_reflow
    // tests/unit/style/custom_properties.rs

    style::thread_state::enter(ThreadState::LAYOUT);

    // Make some CSS
    let stylesheet = make_document_stylesheet(css);

    // Make the real domtree by converting dioxus vnodes
    let markup = RealDom::from_dioxus(markup);

    // servo requires a bunch of locks to be manually held
    // I'm not sure how these work, we should check
    let guard_ = &GLOBAL_STYLE_DATA.shared_lock;
    let (author, user) = (SharedRwLock::new(), SharedRwLock::new());
    let guards = StylesheetGuards {
        author: &author.read(),
        ua_or_user: &user.read(),
    };

    let mut stylist = Stylist::new(
        StyleDevice::new(
            MediaType::screen(),
            QUIRKS_MODE,
            Size2D::new(800., 600.),
            Scale::new(1.0),
        ),
        QUIRKS_MODE,
    );

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

    let shared = build_style_context::<BlitzNode>(
        &stylist,
        guards,
        &snapshots,
        ImmutableOrigin::new_opaque(),
        0.0,
        &animations,
        false,
        &painters,
    );

    // Note that html5ever parses the first node as the document, so we need to unwrap it and get the first child
    // For the sake of this demo, it's always just a single body node, but eventually we will want to construct something like the
    // BoxTree struct that servo uses.
    let root = markup.root();
    let root_element = TDocument::as_node(&root)
        .first_child()
        .unwrap()
        .as_element()
        .unwrap();

    let token = style_traverser::RecalcStyle::pre_traverse(root_element, &shared);
    let traversal = style_traverser::RecalcStyle::new(shared);

    let mut tlc = ThreadLocalStyleContext::new();
    let mut context = StyleContext {
        shared: DomTraversal::<BlitzNode>::shared_context(&traversal),
        thread_local: &mut tlc,
    };

    // force a style manually for each node in the tree
    // I'm not sure if this what you're supposed to do, but eh
    for node_idx in 0..markup.nodes.len() {
        let entry = root.with(node_idx);
        if entry.is_element() {
            resolve_style(
                &mut context,
                entry,
                style::stylist::RuleInclusion::All,
                None,
                None,
            );
        }
    }

    // Style the elements, resolving their data
    // components/style/driver.rs
    // traverse_dom(&traversal, token, None);

    // Now, we should be able to query the elements for their styles
    // Print out the styles for each element
}

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
