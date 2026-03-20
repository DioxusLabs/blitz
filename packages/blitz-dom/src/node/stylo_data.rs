use std::cell::UnsafeCell;
use std::fmt;
use style::data::{ElementDataMut, ElementDataRef, ElementDataWrapper};

/// Interior-mutable wrapper around `Option<ElementDataWrapper>`.
///
/// Encapsulates the `UnsafeCell` so that access sites don't need raw `unsafe` blocks.
/// Safety relies on stylo's single-threaded traversal model: mutations (`set`/`clear`)
/// only happen during exclusive-access phases, and borrows don't overlap with mutations.
pub struct StyloData {
    inner: UnsafeCell<Option<ElementDataWrapper>>,
}

impl Default for StyloData {
    fn default() -> Self {
        Self {
            inner: UnsafeCell::new(None),
        }
    }
}

impl fmt::Debug for StyloData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StyloData").finish_non_exhaustive()
    }
}

impl StyloData {
    /// Whether element data has been initialized.
    pub fn has_data(&self) -> bool {
        unsafe { &*self.inner.get() }.is_some()
    }

    /// Borrow the element data immutably, if present.
    pub fn borrow(&self) -> Option<ElementDataRef<'_>> {
        let opt = unsafe { &*self.inner.get() };
        opt.as_ref().map(|w| w.borrow())
    }

    /// Borrow the element data mutably, if present.
    pub fn borrow_mut(&self) -> Option<ElementDataMut<'_>> {
        let opt = unsafe { &*self.inner.get() };
        opt.as_ref().map(|w| w.borrow_mut())
    }

    /// Set the element data wrapper.
    pub fn set(&self, data: ElementDataWrapper) {
        unsafe { *self.inner.get() = Some(data) };
    }

    /// Clear the element data, returning to the uninitialized state.
    pub fn clear(&self) {
        unsafe { *self.inner.get() = None };
    }
}
