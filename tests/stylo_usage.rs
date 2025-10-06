//! Minimal example of using Stylo
//! TODO: clean up and upstream to stylo repo

// pub use blitz::style_impls::{BlitzNode, RealDom};
// use dioxus::prelude::*;
// use style::{
//     animation::DocumentAnimationSet,
//     context::{QuirksMode, SharedStyleContext},
//     driver,
//     global_style_data::GLOBAL_STYLE_DATA,
//     media_queries::MediaType,
//     media_queries::{Device as StyleDevice, MediaList},
//     selector_parser::SnapshotMap,
//     servo_arc::Arc,
//     shared_lock::{SharedRwLock, StylesheetGuards},
//     stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet},
//     stylist::Stylist,
//     thread_state::ThreadState,
//     traversal::DomTraversal,
//     traversal_flags::TraversalFlags,
// };

// fn main() {
//     let css = r#"
//         h1 {
//             background-color: red;
//         }

//         h2 {
//             background-color: green;
//         }

//         h3 {
//             background-color: blue;
//         }

//         h4 {
//             background-color: yellow;
//         }

//         "#;

//     let nodes = rsx! {
//         h1 { }
//         h2 { }
//         h3 { }
//         h4 { }
//     };

//     let styled_dom = style_lazy_nodes(css, nodes);

//     // print_styles(&styled_dom);
// }

// // pub fn style_lazy_nodes(css: &str, markup: LazyNodes) -> RealDom {
// //     const QUIRKS_MODE: QuirksMode = QuirksMode::NoQuirks;

// //     // Figured out a single-pass system from the servo repo itself:
// //     //
// //     // components/layout_thread_2020/lib.rs:795
// //     //  handle_reflow
// //     // tests/unit/style/custom_properties.rs
// //     style::thread_state::enter(ThreadState::LAYOUT);

// //     // make the guards that we use to thread everything together
// //     let guard = SharedRwLock::new();
// //     let guards = StylesheetGuards {
// //         author: &guard.read(),
// //         ua_or_user: &guard.read(),
// //     };

// //     // Make some CSS
// //     let stylesheet = Stylesheet::from_str(
// //         css,
// //         servo_url::ServoUrl::from_url("data:text/css;charset=utf-8;base64,".parse().unwrap()),
// //         Origin::UserAgent,
// //         Arc::new(guard.wrap(MediaList::empty())),
// //         guard.clone(),
// //         None,
// //         None,
// //         QUIRKS_MODE,
// //         AllowImportRules::Yes,
// //     );

// //     // Make the real domtree by converting dioxus vnodes
// //     let markup = RealDom::from_dioxus(markup);

// //     // Now we need to do what handle_reflow does in servo
// //     // Reflows should be fast due to caching in the Stylist object
// //     //
// //     //
// //     // We can force reflows. Happens in the script/document section
// //     // The browser keeps track of pending restyles itself when attributes are changed.
// //     // When something like val.set_attribute() happens, the pending reflow is inserted into the list.
// //     // Once ticks are finished, the ScriptReflow object is created and sent to the layout thread.
// //     // The layout thread uses the ScriptReflow object to inform itself on what changes need to happen.
// //     // Zooming and touching causes full reflows.
// //     // For this demo we want to do complete reflows (have yet to figure it out)
// //     // But eventually we'll want to queue up modifications and then build the script-reflow type object.
// //     // Unfortunately, this API assumes nodes are backed by pointers which adds some unsafe where we wouldn't want it.
// //     //
// //     // Reflow allows us to specify a dirty root node and a list of nodes to reflow.
// //     //
// //     // Notes:
// //     // - https://developers.google.com/speed/docs/insights/browser-reflow
// //     // - components/script/dom/window.rs:force_reflow
// //     //
// //     // Create a styling context for use throughout the following passes.
// //     // In servo we'd also create a layout context, but since servo isn't updated with the new layout code, we're just using the styling context
// //     // In a different world we'd use both
// //     // Build the stylist object from our screen requirements
// //     // Todo: pull this in from wgpu
// //     let mut stylist = Stylist::new(
// //         StyleDevice::new(
// //             MediaType::screen(),
// //             QUIRKS_MODE,
// //             euclid::Size2D::new(800., 600.),
// //             euclid::Scale::new(1.0),
// //         ),
// //         QUIRKS_MODE,
// //     );

// //     // We have no snapshots on initial render, but we will need them for future renders
// //     let snapshots = SnapshotMap::new();

// //     // Add the stylesheets to the stylist
// //     stylist.append_stylesheet(DocumentStyleSheet(Arc::new(stylesheet)), &guard.read());

// //     // We don't really need to do this, but it's worth keeping it here anyways
// //     stylist.force_stylesheet_origins_dirty(Origin::Author.into());

// //     // Note that html5ever parses the first node as the document, so we need to unwrap it and get the first child
// //     // For the sake of this demo, it's always just a single body node, but eventually we will want to construct something like the
// //     // BoxTree struct that servo uses.
// //     stylist.flush(&guards, Some(markup.root_element()), Some(&snapshots));

// //     // Build the style context used by the style traversal
// //     let context = SharedStyleContext {
// //         traversal_flags: TraversalFlags::empty(),
// //         stylist: &stylist,
// //         options: GLOBAL_STYLE_DATA.options.clone(),
// //         guards,
// //         visited_styles_enabled: false,
// //         animations: (&DocumentAnimationSet::default()).clone(),
// //         current_time_for_animations: 0.0,
// //         snapshot_map: &snapshots,
// //         registered_speculative_painters: &style_impls::RegisteredPaintersImpl,
// //     };

// //     // components/layout_2020/lib.rs:983
// //     println!("------Pre-traversing the DOM tree -----");
// //     let token = style_traverser::RecalcStyle::pre_traverse(markup.root_element(), &context);

// //     // Style the elements, resolving their data
// //     println!("------ Traversing domtree ------",);
// //     let traverser = style_traverser::RecalcStyle::new(context);
// //     driver::traverse_dom(&traverser, token, None);

// //     markup
// // }
