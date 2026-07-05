//! Shared state accessible from both the Rust side (`ScriptDocument`) and the
//! JavaScript side (native functions registered with the Boa `Context`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use blitz_dom::BaseDocument;
use boa_engine::object::JsObject;
use boa_engine::{Finalize, JsData, Trace};

use crate::timers::TimerQueue;

/// Prototype objects for the DOM wrapper classes
pub(crate) struct DomProtos {
    pub node: JsObject,
    pub element: JsObject,
    pub character_data: JsObject,
    pub document: JsObject,
    pub event: JsObject,
    pub style: JsObject,
}

/// An event listener registered via `addEventListener`
#[derive(Clone)]
pub(crate) struct Listener {
    pub callback: JsObject,
    pub capture: bool,
    pub once: bool,
}

pub(crate) type ListenerMap = HashMap<String, Vec<Listener>>;

/// State owned by the script runtime but shared (via `Rc`) with the native
/// functions exposed to JavaScript.
///
/// Note: this struct stores Boa GC handles (`JsObject`s) in ordinary Rust
/// collections. That is sound because Boa GC handles held outside of the GC
/// heap act as roots (they keep their referents alive).
#[derive(Default)]
pub(crate) struct RuntimeState {
    /// Prototypes for DOM wrapper objects. Set once during runtime initialisation.
    pub protos: Option<DomProtos>,
    /// Cache of JS wrapper objects, keyed by node id.
    ///
    /// DOM wrappers must be cached so that a given DOM node is always represented
    /// by the *same* JS object: scripts rely on object identity (`===`) and on
    /// expando properties persisting across accesses.
    pub node_wrappers: HashMap<usize, JsObject>,
    /// Event listeners registered on nodes, keyed by node id then event type.
    pub node_listeners: HashMap<usize, ListenerMap>,
    /// Event listeners registered on `window`.
    pub window_listeners: ListenerMap,
    /// Pending timers (`setTimeout`/`setInterval`/`requestAnimationFrame`)
    pub timers: TimerQueue,
}

impl RuntimeState {
    pub fn protos(&self) -> &DomProtos {
        self.protos
            .as_ref()
            .expect("DOM prototypes not initialised")
    }
}

/// Cloneable handle to the document and the runtime state. This is stored as
/// host-defined data on the Boa [`Context`](boa_engine::Context) so that native
/// functions can access the DOM.
#[derive(Clone, Trace, Finalize, JsData)]
pub(crate) struct DomCtx {
    #[unsafe_ignore_trace]
    pub doc: Rc<RefCell<BaseDocument>>,
    #[unsafe_ignore_trace]
    pub state: Rc<RefCell<RuntimeState>>,
}

impl DomCtx {
    pub fn new(doc: Rc<RefCell<BaseDocument>>) -> Self {
        Self {
            doc,
            state: Rc::new(RefCell::new(RuntimeState::default())),
        }
    }
}
