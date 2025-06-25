//! The core DOM abstraction in Blitz
//!
//! This crate implements a headless DOM designed to emebedded in and "driven" by external code.
//!
//! It includes:
//!  - A DOM tree respresentation
//!  - CSS parsing and resolution
//!  - Layout
//!  - Event handling
//!
//! The following functionality is not included within blitz-dom. However there are extension points that can be used to implement this
//! functionality using either another `blitz-*` crate or a custom implementation:
//!  - Networking (see blitz_net)
//!  - Windowing or an event loop (see blitz_shell)
//!  - Rendering (see `blitz_paint`)
//!
//! `blitz-dom` has a native Rust API that is designed for higher-level abstractions to be built on top (although it can also be used directly).
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
pub use node::{Attribute, ElementData, Node, NodeData, TextNodeData};
pub use parley::FontContext;
pub use style::Atom;
pub use style::invalidation::element::restyle_hints::RestyleHint;
pub type SelectorList = selectors::SelectorList<style::selector_parser::SelectorImpl>;
pub use events::{EventDriver, EventHandler, NoopEventHandler};
