use blitz_traits::node_id::NodeId;

/// One inline `<svg>` fragment: the parsed, ready-to-paint state built from
/// a root `<svg>` element (`SpecialElementData::SvgRoot`).
///
/// Built by [`rebuild_svg_fragments`](super::rebuild_svg_fragments) *after*
/// Taffy layout runs, because `viewBox` and percentage geometry resolve
/// against the root's content-box size, which isn't known any earlier.
#[derive(Debug)]
pub struct SvgContext {
    /// The `<svg>` element this fragment was built from.
    pub root: NodeId,
    /// The root `<svg>`'s content-box size in CSS px:
    /// The viewport that `viewBox` and percentage geometry resolve against.
    pub viewport: kurbo::Size,
}
