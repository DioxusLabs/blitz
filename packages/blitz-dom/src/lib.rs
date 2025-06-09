//! Blitz-dom
//!
//! This crate implements a simple ECS-based DOM, with a focus on performance and ease of use. We don't attach bindings
//! to languages here, simplifying the API and decreasing code size.
//!
//! The goal behind this crate is that any implementor can interact with the DOM and render it out using any renderer
//! they want.
//!
//! ## Feature flags
//!  - `default`: Enables the features listed below.
//!  - `tracing`: Enables tracing support.

pub const DEFAULT_CSS: &str = include_str!("../assets/default.css");
pub(crate) const BULLET_FONT: &[u8] = include_bytes!("../assets/moz-bullet-font.otf");

/// The DOM implementation.
///
/// This is the primary entry point for this crate.
mod document;

/// The nodes themsleves, and their data.
///
/// todo: we want this to use ECS, but we're not done with the design yet.
pub mod node;

mod debug;
mod events;
mod form;
/// Integration of taffy and the DOM.
mod layout;
mod mutator;
mod query_selector;
/// Implementations that interact with servo's style engine
mod stylo;
mod stylo_to_cursor_icon;
mod stylo_to_parley;
mod traversal;

pub mod net;
pub mod util;

#[cfg(feature = "accessibility")]
mod accessibility;

pub use document::{BaseDocument, Document};
pub use markup5ever::{
    LocalName, Namespace, NamespaceStaticSet, Prefix, PrefixStaticSet, QualName, local_name,
    namespace_prefix, namespace_url, ns,
};
pub use mutator::DocumentMutator;
pub use node::{Attribute, ElementNodeData, Node, NodeData, TextNodeData};
pub use parley::FontContext;
pub use style::Atom;
pub use style::invalidation::element::restyle_hints::RestyleHint;
pub type SelectorList = selectors::SelectorList<style::selector_parser::SelectorImpl>;
pub use events::{EventDriver, EventHandler, NoopEventHandler};
