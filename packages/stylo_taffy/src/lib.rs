//! Conversion functions from Stylo types to Taffy types
//!
//! This crate is an implementation detail of [`blitz-dom`](https://docs.rs/blitz-dom), but can also be
//! used standalone, and serves as useful reference for anyone wanting to integrate [`stylo`](::style) with [`taffy`]

mod wrapper;
pub use wrapper::{StyleFlags, TaffyStyloStyle};

pub mod convert;
#[doc(inline)]
pub use convert::to_taffy_style;
#[doc(inline)]
pub use convert::to_taffy_style_in;

pub mod writing_mode;
pub use style::logical_geometry::WritingMode;
pub use writing_mode::WritingModeExt;

pub use style::Atom;
