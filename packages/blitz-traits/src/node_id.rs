//! Versioned node identifier shared between the Blitz crates.

/// A versioned identifier for a node in a Blitz DOM tree.
///
/// A `NodeId` packs a 32-bit slot index (low bits) and a 32-bit version
/// (high bits). Node storage bumps a slot's version when the slot is
/// reused, so ids referring to a dropped node no longer resolve (lookups
/// return `None` rather than aliasing the new node occupying the slot).
///
/// The `Default` value is a null id which never resolves to a node.
#[derive(Copy, Clone, Default, Eq, PartialEq, Hash)]
pub struct NodeId(u64);

impl NodeId {
    /// Convert this id to its raw `u64` representation (index + version).
    ///
    /// The value round-trips through [`NodeId::from_u64`]. This is useful for
    /// interop with APIs which use integer ids (e.g. Taffy or AccessKit).
    #[inline(always)]
    pub fn as_u64(self) -> u64 {
        self.0
    }

    /// Reconstruct a `NodeId` from the raw `u64` representation produced by
    /// [`NodeId::as_u64`].
    ///
    /// Passing a value that did not come from `as_u64` will produce an id
    /// which fails to resolve (it will not alias an unrelated live node).
    #[inline(always)]
    pub fn from_u64(raw: u64) -> Self {
        Self(raw)
    }

    /// The slot index part of this id.
    #[inline(always)]
    fn index(self) -> u32 {
        self.0 as u32
    }

    /// The version part of this id.
    #[inline(always)]
    fn version(self) -> u32 {
        (self.0 >> 32) as u32
    }
}

/// Ordered by slot index first, then by version, so that ordering roughly
/// matches node creation order (as with the previous `Slab`-backed storage)
/// rather than being dominated by the version bits.
impl Ord for NodeId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.index(), self.version()).cmp(&(other.index(), other.version()))
    }
}

impl PartialOrd for NodeId {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl std::fmt::Debug for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NodeId({}v{})", self.index(), self.version())
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
