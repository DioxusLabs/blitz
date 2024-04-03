//! Blitz-dom
//!
//! This crate implements a simple ECS-based DOM, with a focus on performance and ease of use. We don't attach bindings
//! to languages here, simplifying the API and decreasing code size.
//!
//! The goal behind this crate is that any implementor can interact with the DOM and render it out using any renderer
//! they want.

/// The DOM implementation.
///
/// This is the primary entry point for this crate.
pub mod document;

/// An implementation for Html5ever's sink trait, allowing us to parse HTML into a DOM.
pub mod htmlsink;

/// Integration of taffy and the DOM.
pub mod layout;

/// A collection of methods for manipulating the DOM.
pub mod mutation;

/// The nodes themsleves, and their data.
///
/// todo: we want this to use ECS, but we're not done with the design yet.
pub mod node;

/// Implementations that interact with servo's style engine
pub mod stylo;

/// Conversions from Stylo types to Taffy types
pub mod stylo_to_taffy;

pub mod image;
/// Utilities for laying out and measuring text
pub mod text;

pub mod util;

pub use document::Document;
pub use htmlsink::DocumentHtmlParser;
pub use node::Node;
