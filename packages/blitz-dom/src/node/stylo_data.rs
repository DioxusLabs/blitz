use std::fmt;
use std::ops::Deref;
use std::{cell::UnsafeCell, ops::DerefMut};
use style::data::{ElementDataMut, ElementDataRef, ElementDataWrapper};
use style::servo_arc::Arc;

use crate::layout::damage::ALL_DAMAGE;

/// Interior-mutable wrapper around `Option<ElementDataWrapper>`.
///
/// Encapsulates the `UnsafeCell` so that access sites don't need raw `unsafe` blocks.
///
/// Safety relies on:
///   - Regular static-borrow checking for regular access, and on:
///   - Stylo having exclusive access to nodes during style traversals
///   - Stylo's thread-safe traversal model: `init`/`clear` only happen during exclusive-access phases
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

impl Deref for StyloData {
    type Target = Option<ElementDataWrapper>;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.inner.get() }
    }
}

impl DerefMut for StyloData {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.get_mut()
    }
}

impl StyloData {
    /// Whether element data has been initialized.
    pub fn has_data(&self) -> bool {
        unsafe { &*self.inner.get() }.is_some()
    }

    /// Borrow the element data immutably, if present.
    pub fn get(&self) -> Option<ElementDataRef<'_>> {
        self.as_ref().map(|w| w.borrow())
    }

    /// Borrow the element data mutably, if present.
    pub fn get_mut(&mut self) -> Option<ElementDataMut<'_>> {
        self.as_mut().map(|w| w.borrow_mut())
    }

    /// Initialize the element data ready for use (if it is not already initialized)
    pub fn ensure_init_mut(&mut self) -> ElementDataMut<'_> {
        // SAFETY:
        // If we have exclusive access to self (implied by &mut self) then it safe to mutate self.
        unsafe { self.ensure_init() }
    }

    pub fn primary_styles(&self) -> Option<StyleDataRef<'_>> {
        let stylo_element_data = self.get();
        if stylo_element_data
            .as_ref()
            .and_then(|d| d.styles.get_primary())
            .is_some()
        {
            Some(StyleDataRef(self.get().unwrap()))
        } else {
            None
        }
    }

    /// Get a mutable reference to the data
    pub unsafe fn unsafe_stylo_only_mut(&self) -> Option<ElementDataMut<'_>> {
        let opt = unsafe { &mut *self.inner.get() };
        opt.as_mut().map(|w| w.borrow_mut())
    }

    /// Initialize the element data ready for use (if it is not already initialized)
    ///
    /// SAFETY:
    /// There must be no outstanding borrows to this container or anything contained within it
    /// when this method is called
    pub unsafe fn ensure_init(&self) -> ElementDataMut<'_> {
        if !self.has_data() {
            unsafe { *self.inner.get() = Some(ElementDataWrapper::default()) };
            let mut data_mut = unsafe { self.unsafe_stylo_only_mut() }.unwrap();
            data_mut.damage = ALL_DAMAGE;
            data_mut
        } else {
            unsafe { self.unsafe_stylo_only_mut() }.unwrap()
        }
    }

    /// Clear the element data, returning to the uninitialized state.
    ///
    /// SAFETY:
    /// There must be no outstanding borrows to this container or anything contained within it
    /// when this method is called
    pub unsafe fn clear(&self) {
        unsafe { *self.inner.get() = None };
    }
}

pub struct StyleDataRef<'a>(ElementDataRef<'a>);

impl Deref for StyleDataRef<'_> {
    type Target = Arc<style::properties::ComputedValues>;

    fn deref(&self) -> &Self::Target {
        self.0.styles.get_primary().unwrap()
    }
}
