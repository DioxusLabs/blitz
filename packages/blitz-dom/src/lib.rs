//! The core DOM abstraction in Blitz
//!
//! This crate implements a flexible headless DOM ([`BaseDocument`]), which is designed to emebedded in and "driven" by external code. Most users will want
//! to use a wrapper:
//!
//!  - [`HtmlDocument`](https://docs.rs/blitz-html/latest/blitz_html/struct.HtmlDocument.html) from the [blitz-html](https://docs.rs/blitz-html) crate.
//!    Allows you to parse HTML (or XHTML) into a Blitz [`BaseDocument`], and can be combined with a markdown-to-html converter like [comrak](https://docs.rs/comrak)
//!    or [pulldown-cmark](https://docs.rs/pulldown-cmark) to render/process markdown.
//!  - [`DioxusDocument`](https://docs.rs/dioxus-native/latest/dioxus_native/struct.DioxusDocument.html) from the [dioxus-native](https://docs.rs/dioxus-native) crate.
//!    Combines a [`BaseDocument`] with a Dioxus `VirtualDom` to enable dynamic rendering and event handling.
//!
//! It includes: A DOM tree respresentation, CSS parsing and resolution, layout and event handling. Additional functionality is available in
//! separate crates, including html parsing ([blitz-html](https://docs.rs/blitz-html)), networking ([blitz-net](https://docs.rs/blitz-html)),
//! rendering ([blitz-paint](https://docs.rs/blitz-paint)) and windowing ([blitz-shell](https://docs.rs/blitz-shell)).
//!
//! Most of the functionality in this crates is provided through the  struct.
//!
//! `blitz-dom` has a native Rust API that is designed for higher-level abstractions to be built on top (although it can also be used directly).
//!
//! The goal behind this crate is that any implementor can interact with the DOM and render it out using any renderer
//! they want.
//!

// TODO: Document features
// ## Feature flags
//  - `default`: Enables the features listed below.
//  - `tracing`: Enables tracing support.

pub const DEFAULT_CSS: &str = include_str!("../assets/default.css");
pub(crate) const BULLET_FONT: &[u8] = include_bytes!("../assets/moz-bullet-font.otf");

const INCREMENTAL: bool = cfg!(feature = "incremental");
const NON_INCREMENTAL: bool = !INCREMENTAL;

/// The DOM implementation.
///
/// This is the primary entry point for this crate.
mod document;

/// The nodes themsleves, and their data.
pub mod node;

mod config;
mod debug;
mod events;
mod font_metrics;
mod form;
mod html;
/// Integration of taffy and the DOM.
mod layout;
mod mutator;
mod query_selector;
/// Implementations that interact with servo's style engine
mod stylo;
mod stylo_to_cursor_icon;
mod stylo_to_parley;
mod traversal;
mod url;

pub mod net;
pub mod util;

#[cfg(feature = "accessibility")]
mod accessibility;

pub use config::DocumentConfig;
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
pub use html::{DummyHtmlParserProvider, HtmlParserProvider};
pub use util::Point;
