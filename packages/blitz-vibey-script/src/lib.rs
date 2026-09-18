//! JavaScript execution on top of Blitz
//!
//! This crate implements a [`ScriptDocument`]: a wrapper around a [`BaseDocument`](blitz_dom::BaseDocument)
//! which can execute the JavaScript contained in (or referenced by) the document's `<script>` tags
//! using the [Boa](https://boajs.dev) JavaScript engine, and which exposes JavaScript DOM APIs
//! (`document`, elements, events, timers, etc) backed by `blitz-dom` to the scripts it runs.
//!
//! It is capable of running real-world JavaScript frameworks such as [Preact](https://preactjs.com/).
//!
//! ### Example
//!
//! ```rust
//! use blitz_vibey_script::ScriptDocument;
//! use blitz_dom::DocumentConfig;
//!
//! let mut doc = ScriptDocument::from_html(
//!     r#"
//!         <div id="root"></div>
//!         <script>
//!             const el = document.createElement("h1");
//!             el.textContent = "Hello from JS";
//!             document.getElementById("root").appendChild(el);
//!         </script>
//!     "#,
//!     DocumentConfig::default(),
//! );
//! doc.execute_scripts();
//! ```

#![allow(clippy::collapsible_if)]

mod clock;
mod document;
mod dom;
mod event_handler;
mod fetch;
mod inner_text;
mod runtime;
mod state;
mod timers;

pub use document::ScriptDocument;
pub use fetch::{DefaultScriptFetcher, FetchError, ScriptFetcher};

/// Host-side helpers for native functions installed into the script context.
///
/// A host that adds its own bindings (e.g. a `<canvas>` 2D context) needs to
/// get from a JS node wrapper back to the DOM node it stands for, and to the
/// document itself, from inside a native call.
pub mod host {
    use std::cell::RefCell;
    use std::rc::Rc;

    use blitz_dom::{BaseDocument, NodeId};
    use boa_engine::{Context, JsValue};

    /// The DOM node a JS wrapper object stands for, if it is one.
    pub fn node_id_of(value: &JsValue) -> Option<NodeId> {
        crate::dom::node_id_of_value(value)
    }

    /// The document behind this script context.
    pub fn document_of(context: &mut Context) -> Option<Rc<RefCell<BaseDocument>>> {
        crate::dom::dom_ctx(context)
            .ok()
            .map(|ctx| Rc::clone(&ctx.doc))
    }
}
