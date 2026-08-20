//! Core geometry-tree types for first-party inline SVG rendering.
//!
//! An [`SvgContext`] is built once per root `<svg>` fragment (rebuilt wholesale
//! on `CONSTRUCT_SVG` damage, see `layout::damage`) and holds a flat,
//! render-order list of [`SvgNode`]s with precomputed CTMs. Painting is then a
//! linear scan with no tree recursion or matrix chaining.

use std::collections::HashMap;

use blitz_traits::node_id::NodeId;
use kurbo::{Affine, BezPath, Rect, Size};
use style::Atom;

use super::text::TextAnchor;
use crate::node::TextBrush;

/// A shaped `<text>`/`<tspan>` run plus its anchor alignment.
pub struct TextRun {
    pub layout: parley::Layout<TextBrush>,
    pub anchor: TextAnchor,
}

/// `preserveAspectRatio` alignment keyword.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    None,
    XMinYMin,
    XMidYMin,
    XMaxYMin,
    XMinYMid,
    XMidYMid,
    XMaxYMid,
    XMinYMax,
    XMidYMax,
    XMaxYMax,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeetOrSlice {
    Meet,
    Slice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreserveAspectRatio {
    pub align: Align,
    pub meet_or_slice: MeetOrSlice,
}

impl Default for PreserveAspectRatio {
    fn default() -> Self {
        Self {
            align: Align::XMidYMid,
            meet_or_slice: MeetOrSlice::Meet,
        }
    }
}

/// A fully-resolved inline-SVG fragment, rooted at one `<svg>` element that establishes a CSS box.
pub struct SvgContext {
    /// DOM id of the root `<svg>` element.
    pub root: NodeId,
    /// CSS content-box size of the root `<svg>`, in CSS px.
    pub viewport: Size,
    /// Parsed `viewBox` attribute, if present.
    pub viewbox: Option<Rect>,
    pub preserve_aspect_ratio: PreserveAspectRatio,
    pub root_ctm: Affine,
    /// Flattened render-order node list. Excludes elements inside non-rendered containers,
    /// those are reachable only via `id_map`.
    pub nodes: Vec<SvgNode>,
    /// Covers every id in the fragment, including inside `display:none` containers
    /// which never get an [`SvgNode`] of their own (V24).
    pub id_map: HashMap<Atom, NodeId>,
}

pub struct SvgNode {
    pub dom_id: NodeId,
    pub parent: Option<u32>,
    /// This node's own user space -> viewport space.
    pub ctm: Affine,
    pub kind: SvgNodeKind,
    /// Object bounding box (fill geometry only, no stroke), in this node's own user space.
    /// Used as the reference box for `objectBoundingBox` paint-server units, `transform-box`,
    /// and geometry percentages that resolve against it.
    pub bbox: Rect,
}

pub enum SvgNodeKind {
    /// `<g>`, `<a>`, `<svg>` (nested viewport establisher), `<symbol>`
    /// instance root, or any container with no geometry of its own.
    Group,
    /// A filled/stroked shape: `<rect>`, `<circle>`, `<ellipse>`, `<line>`,
    /// `<polyline>`, `<polygon>`, `<path>`. Path is in the node's own user
    /// space.
    Shape(BezPath),
    /// `<text>` / standalone `<tspan>` run, already shaped.
    Text(Box<TextRun>),
    /// `<image>` with a resolved raster source (`None` while the fetch is
    /// still pending.
    Image,
    /// `<foreignObject>`: re-enters normal HTML layout/paint. Carries
    /// the DOM id of the `<foreignObject>` element itself so paint can
    /// re-enter `render_element`.
    ForeignObject,
    /// `<use>` instance root. The target subtree is shadow-expanded into further
    /// [`SvgNode`]s appended to `nodes` with `parent` pointing back at this
    /// node, so painting remains a flat linear scan.
    Use { target: NodeId },
}
