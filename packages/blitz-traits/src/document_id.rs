//! Unique document identifier shared between the Blitz crates.

use std::fmt;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A process-wide unique identifier for a document.
///
/// Ids can only be created with [`DocumentId::new`], which never returns the
/// same id twice, so an id can be allocated before the document it will
/// identify is created (see `DocumentConfig::id`) without risking a
/// collision. The numeric value can be read with [`DocumentId::get`], but a
/// `DocumentId` can't be constructed from one.
#[derive(Copy, Clone, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct DocumentId(NonZeroUsize);

impl DocumentId {
    /// Allocate a new unique id
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        Self(NonZeroUsize::new(id).expect("document id overflow"))
    }

    /// The numeric value of this id
    #[inline(always)]
    pub fn get(self) -> usize {
        self.0.get()
    }
}

impl fmt::Debug for DocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DocumentId({})", self.0)
    }
}

impl fmt::Display for DocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::DocumentId;

    #[test]
    fn ids_are_unique() {
        let a = DocumentId::new();
        let b = DocumentId::new();
        assert_ne!(a, b);
        assert!(b.get() > a.get());
    }
}
