//! Paint hook: let an app draw on elements while the document is painted.
//!
//! The painter walks the document in paint order and works out where each element lands. With
//! a hook installed ([`BaseDocument::set_paint_hook`]) it calls the hook before, at and after
//! each step of every element, with the element ([`PaintEl`]) and a scene to draw into. The
//! hook adds drawing, and can take over a step ([`PaintHook::replaces`]).
//!
//! ```ignore
//! struct Highlight;
//!
//! impl PaintHook for Highlight {
//!     fn paint(&self, point: PaintPoint, el: &PaintEl<'_>, scene: &mut Scene) {
//!         let node = el.node();
//!         let on = el.document().resolved_style_value(node.id, "--highlight");
//!         if point == PaintPoint::After(Step::Border) && !on.is_empty() {
//!             let style = node.primary_styles().unwrap();
//!             let boxes = blitz_paint::create_css_rect(&style, node.final_layout(), 1.0);
//!             let white = Color::from_rgba8(255, 255, 255, 40);
//!             let shape = boxes.padding_box_path();
//!             scene.fill(Fill::NonZero, Affine::IDENTITY, white, None, &shape);
//!         }
//!     }
//! }
//!
//! doc.set_paint_hook(Highlight);
//! ```
//!
//! This is the whole interface: one hook per document, called for every element. Selectors,
//! lists of effects and shorter ways to draw are built on top of it, outside this crate.
//!
//! # When
//!
//! The painter draws an element in four steps ([`Step`]). The hook is called before and after
//! each one, and at a step it takes over, in place of the painter's own drawing
//! ([`PaintPoint`]):
//!
//! ```text
//!   Before(Shadow)
//! outline, outer box-shadow        or At(Shadow)
//!   After(Shadow)
//! ---- the element's layer starts: opacity, filter, clip-path ----
//!   Before(Background)
//! background                       or At(Background)
//!   After(Background)
//! inset box-shadow
//!   Before(Border)
//! border                           or At(Border)
//!   After(Border), Before(Content)
//! image, text, children            or At(Content)
//!   After(Content)
//! scrollbars
//! ```
//!
//! Drawing at the `Shadow` points is outside the element's layer: it is not cut to the
//! element's box and not faded by its `opacity`. Use them for anything larger than the element,
//! such as a glow. Everything from `Before(Background)` on is inside the layer.
//!
//! The hook is called for boxes. Text inside a line (a plain `<span>`) is painted as part of
//! its paragraph and gets no call; inline images and form controls do.
//!
//! # What
//!
//! The scene is a recorded [`anyrender::Scene`], the same kind the painter and custom widgets
//! draw with. Everything is in CSS pixels, in the element's own space: the origin is
//! the top-left corner of its border box. The painter applies the element's position,
//! transform and the screen scale.
//!
//! # The element and the rest of the document
//!
//! Layout and style are finished before painting starts. [`PaintEl::node`] and
//! [`PaintEl::document`] give them through the document's own interface: attributes, computed
//! style, custom properties (`resolved_style_value`), selectors (`query_selector`), parents and
//! children, and where another element is (`get_client_bounding_rect`). The painter's
//! `create_css_rect` gives an element's border, padding and content boxes with their corners.
//!
//! # State and animation
//!
//! The hook is called through a shared reference on every painted frame. Keep changing state
//! in a `Cell` or `RefCell`. The painter only runs when something asks for a frame; an
//! animating hook asks for the next one with the document's `shell_provider.request_redraw()`.

use std::any::Any;

use anyrender::Scene;
use kurbo::Affine;

use crate::{BaseDocument, Node};

// The crates a hook draws with
pub use {anyrender, kurbo, peniko};

/// A part of an element that the painter draws. The hook is called before and after each one
/// ([`PaintPoint`]) and can take one over ([`PaintHook::replaces`]).
///
/// Listed in the order the painter runs them.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Step {
    /// The `box-shadow`, outer and inset. The outline is drawn with the outer shadow, is not
    /// part of the step and is always drawn.
    Shadow,
    /// The background: colour, images and gradients.
    Background,
    /// The border.
    Border,
    /// Image, text and children.
    Content,
}

/// A moment in an element's paint sequence at which the hook is called: before one of the
/// element's steps, at it, or after it.
///
/// `After(Step::Border)` and `Before(Step::Content)` are the same moment, called one after
/// the other. The `Shadow` points are outside the element's opacity, filter and clip layers.
/// The `Content` points are not scrolled with the content and not cut by `overflow`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum PaintPoint {
    /// Just before the step. Drawing here is under it.
    Before(Step),
    /// In the place of the step. Only called for a step the hook takes over
    /// ([`PaintHook::replaces`]), which the painter then does not draw.
    At(Step),
    /// Just after the step. Drawing here is over it.
    After(Step),
}

/// Code that draws on elements while the document is painted. See the module documentation.
pub trait PaintHook: Any {
    /// Called once for each painted element, before any point. Return `false` to receive no
    /// [`replaces`](Self::replaces) or [`paint`](Self::paint) calls for this element.
    ///
    /// Elements nest: the calls for an element's children arrive between its
    /// `Before(Content)` and `After(Content)` points. [`end_element`](Self::end_element)
    /// closes each `begin_element`, also when it returned `false`. A hook that works
    /// something out per element keeps it on a stack between the two.
    fn begin_element(&self, el: &PaintEl<'_>) -> bool {
        let _ = el;
        true
    }

    /// Whether the hook draws `step` of this element itself. The painter then leaves that step
    /// out and calls the hook at [`PaintPoint::At`] in its place.
    ///
    /// May be asked more than once per step while an element is painted, and must give the
    /// same answer each time. Replacing [`Step::Content`] also leaves
    /// out the element's children. For [`Step::Shadow`] the place is that of the outer shadow;
    /// the inset shadow's place is `After(Background)`. Layout, hit testing and the element's
    /// layers (`opacity`, `filter`, `clip-path`) are not affected.
    fn replaces(&self, step: Step, el: &PaintEl<'_>) -> bool {
        let _ = (step, el);
        false
    }

    /// Draw on `el` at `point`, into `scene`. The scene's drawing calls come from the
    /// [`anyrender::PaintScene`] trait. Every layer that is pushed must be popped before
    /// returning.
    fn paint(&self, point: PaintPoint, el: &PaintEl<'_>, scene: &mut Scene);

    /// Called once for each painted element, after its last point.
    fn end_element(&self, el: &PaintEl<'_>) {
        let _ = el;
    }
}

impl BaseDocument {
    /// Install the paint hook, replacing any earlier one.
    pub fn set_paint_hook(&mut self, hook: impl PaintHook) {
        self.paint_hook = Some(Box::new(hook));
        self.shell_provider.request_redraw();
    }

    /// Remove the paint hook and return it.
    pub fn take_paint_hook(&mut self) -> Option<Box<dyn PaintHook>> {
        self.shell_provider.request_redraw();
        self.paint_hook.take()
    }

    /// The installed paint hook.
    pub fn paint_hook(&self) -> Option<&dyn PaintHook> {
        self.paint_hook.as_deref()
    }

    /// The installed paint hook, for changing it in place. Asks for a redraw.
    pub fn paint_hook_mut(&mut self) -> Option<&mut dyn PaintHook> {
        self.shell_provider.request_redraw();
        self.paint_hook.as_deref_mut()
    }
}

/// The element being painted.
pub struct PaintEl<'a> {
    doc: &'a BaseDocument,
    node: &'a Node,
    transform: Affine,
    scale: f64,
}

impl<'a> PaintEl<'a> {
    /// Called by the painter.
    pub fn new(doc: &'a BaseDocument, node: &'a Node, transform: Affine, scale: f64) -> Self {
        Self {
            doc,
            node,
            transform,
            scale,
        }
    }

    /// The element's node: its id, attributes, computed style, parent and children.
    pub fn node(&self) -> &'a Node {
        self.node
    }

    /// The document being painted, for reaching other elements.
    pub fn document(&self) -> &'a BaseDocument {
        self.doc
    }

    /// Maps the element's own space (CSS pixels) to the window (physical pixels). Its inverse
    /// brings a point in the window, such as where a press landed, into the element's space.
    pub fn transform(&self) -> Affine {
        self.transform * Affine::scale(self.scale)
    }

    /// Physical pixels per CSS pixel.
    pub fn scale(&self) -> f64 {
        self.scale
    }
}
