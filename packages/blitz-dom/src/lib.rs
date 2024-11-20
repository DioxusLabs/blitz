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
pub mod document;

pub mod html_document;
/// An implementation for Html5ever's sink trait, allowing us to parse HTML into a DOM.
pub mod htmlsink;

/// Integration of taffy and the DOM.
pub mod layout;

/// The nodes themsleves, and their data.
///
/// todo: we want this to use ECS, but we're not done with the design yet.
pub mod node;

/// Implementations that interact with servo's style engine
pub mod stylo;

pub mod stylo_to_parley;

pub mod image;

pub mod util;

pub mod debug;

pub mod events;

pub mod net;

pub mod viewport;

pub use document::{Document, DocumentLike};
pub use html5ever::{
    local_name, namespace_prefix, namespace_url, ns, Namespace, NamespaceStaticSet, Prefix,
    PrefixStaticSet, QualName,
};
pub use html_document::HtmlDocument;
pub use htmlsink::DocumentHtmlParser;
pub use node::{ElementNodeData, Node, NodeData, TextNodeData};
pub use string_cache::Atom;
pub use viewport::Viewport;
