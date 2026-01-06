//! UIKit renderer for blitz-dom
//!
//! This crate provides a native UIKit renderer that maps blitz-dom nodes to iOS UIKit views.
//! It provides a React Native-like experience for rendering HTML/CSS on iOS.
//!
//! # Architecture
//!
//! The renderer walks the DOM tree from BaseDocument and creates corresponding UIKit views:
//! - `<div>`, `<section>`, etc. → `UIView`
//! - `<p>`, `<span>`, `<h1>` → `UILabel`
//! - `<button>` → `UIButton`
//! - `<input type="text">` → `UITextField`
//! - `<input type="checkbox">` → `UISwitch`
//! - `<img>` → `UIImageView`
//!
//! Layout is computed by Taffy (CSS flexbox/grid) and applied as UIView frames.
//! Events from UIKit are bridged back to blitz-dom's event system.

mod elements;
mod events;
mod style;
mod sync;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use blitz_dom::BaseDocument;
use objc2::rc::Retained;
use objc2_foundation::MainThreadMarker;
use objc2_ui_kit::UIView;
use rustc_hash::FxHashMap;

pub use elements::ElementType;
pub use events::EventSender;

/// Entry in the view map tracking a UIView and its metadata
pub struct ViewEntry {
    /// The actual UIView (retained)
    pub view: Retained<UIView>,
    /// The element type for this view
    pub element_type: ElementType,
    /// Generation counter for change detection during sync
    pub generation: u64,
}

/// The main renderer that bridges BaseDocument to UIKit views.
///
/// # Usage
///
/// ```ignore
/// let mtm = MainThreadMarker::new().unwrap();
/// let doc = Rc::new(RefCell::new(BaseDocument::new(...)));
/// let root_view = UIView::new(mtm);
///
/// let mut renderer = UIKitRenderer::new(doc, root_view, mtm);
/// renderer.sync(); // Initial render
///
/// // After DOM changes:
/// renderer.sync(); // Update UIKit views to match DOM
/// ```
pub struct UIKitRenderer {
    /// Reference to the document being rendered
    doc: Rc<RefCell<BaseDocument>>,

    /// The root UIView that contains all rendered content
    root_view: Retained<UIView>,

    /// Mapping from blitz NodeId to UIView
    view_map: FxHashMap<usize, ViewEntry>,

    /// MainThreadMarker required for UIKit operations
    mtm: MainThreadMarker,

    /// Current viewport scale (points per CSS pixel)
    scale: f64,

    /// Generation counter incremented each sync
    generation: u64,

    /// Event sender for dispatching UIKit events back to blitz-dom
    event_sender: EventSender,
}

impl UIKitRenderer {
    /// Create a new UIKit renderer.
    ///
    /// # Arguments
    ///
    /// * `doc` - The BaseDocument to render
    /// * `root_view` - The root UIView to add rendered content to
    /// * `mtm` - MainThreadMarker proving we're on the main thread
    pub fn new(
        doc: Rc<RefCell<BaseDocument>>,
        root_view: Retained<UIView>,
        mtm: MainThreadMarker,
    ) -> Self {
        Self {
            doc,
            root_view,
            view_map: FxHashMap::default(),
            mtm,
            scale: 1.0, // TODO: Get from UIScreen
            generation: 0,
            event_sender: EventSender::new(),
        }
    }

    /// Set the scale factor (points per CSS pixel).
    ///
    /// This should match `UIScreen.mainScreen.scale` for proper rendering.
    pub fn set_scale(&mut self, scale: f64) {
        self.scale = scale;
    }

    /// Get the current scale factor.
    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// Get the MainThreadMarker for creating UIKit objects.
    pub fn main_thread_marker(&self) -> MainThreadMarker {
        self.mtm
    }

    /// Get a reference to the root view.
    pub fn root_view(&self) -> Retained<UIView> {
        self.root_view.clone()
    }

    /// Get the event sender for dispatching events.
    pub fn event_sender(&self) -> &EventSender {
        &self.event_sender
    }

    /// Synchronize the UIView hierarchy with the DOM tree.
    ///
    /// This walks the DOM tree and creates, updates, or removes UIViews
    /// to match the current DOM state.
    pub fn sync(&mut self) {
        sync::sync_tree(self);
    }

    /// Get a view by node ID, if it exists.
    pub fn get_view(&self, node_id: usize) -> Option<&ViewEntry> {
        self.view_map.get(&node_id)
    }

    /// Insert a view entry for a node.
    pub(crate) fn insert_view(&mut self, node_id: usize, entry: ViewEntry) {
        self.view_map.insert(node_id, entry);
    }

    /// Remove a view entry for a node.
    pub(crate) fn remove_view(&mut self, node_id: usize) -> Option<ViewEntry> {
        self.view_map.remove(&node_id)
    }

    /// Get mutable access to the view map.
    pub(crate) fn view_map_mut(&mut self) -> &mut FxHashMap<usize, ViewEntry> {
        &mut self.view_map
    }

    /// Get the current generation counter.
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// Increment and get the next generation counter.
    pub(crate) fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    /// Get a reference to the document.
    pub(crate) fn doc(&self) -> Rc<RefCell<BaseDocument>> {
        self.doc.clone()
    }
}
