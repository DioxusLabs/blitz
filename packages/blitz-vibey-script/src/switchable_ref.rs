//! A reference to a JS object whose strength can be switched at runtime.
//!
//! A `JsObject` handle held outside the GC heap acts as a root (it keeps its
//! referent alive); a `WeakJsObject` lets the referent be collected. Wrapper
//! cache entries start strong so that event listeners registered on them are
//! never lost, and can be switched to weak to hand the wrapper back to the
//! GC.

use boa_engine::object::{JsObject, WeakJsObject};

/// A reference to a JS object whose strength can be switched at runtime.
pub(crate) enum SwitchableRef {
    /// Keeps the object alive: the handle is a GC root.
    Strong(JsObject),
    /// Lets the object be collected; `get` returns `None` once it is.
    Weak(WeakJsObject),
}

impl SwitchableRef {
    pub(crate) fn new(obj: JsObject) -> Self {
        Self::Strong(obj)
    }

    /// Retrieve the JS object. Returns `None` if it has been collected
    /// (only possible in weak mode).
    pub(crate) fn get(&self) -> Option<JsObject> {
        match self {
            Self::Strong(obj) => Some(obj.clone()),
            Self::Weak(weak) => weak.upgrade(),
        }
    }

    /// Whether the object is still alive.
    pub(crate) fn is_alive(&self) -> bool {
        match self {
            Self::Strong(..) => true,
            Self::Weak(weak) => weak.is_upgradable(),
        }
    }

    /// Switch to weak. No-op if already weak.
    pub(crate) fn make_weak(&mut self) {
        if let Self::Strong(obj) = self {
            *self = Self::Weak(WeakJsObject::new(obj));
        }
    }

    /// Switch to strong. No-op if already strong. Returns `false` if the
    /// object has already been collected.
    pub(crate) fn make_strong(&mut self) -> bool {
        match self {
            Self::Strong(_) => true,
            Self::Weak(weak) => match weak.upgrade() {
                Some(obj) => {
                    *self = Self::Strong(obj);
                    true
                }
                None => false,
            },
        }
    }
}
