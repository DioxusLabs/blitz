//! Conversion functions from Stylo types to Taffy types
//!
//! This crate is an implementation detail of [`blitz-dom`](https://docs.rs/blitz-dom), but can also be
//! used standalone, and serves as useful reference for anyone wanting to integrate [`stylo`](::style) with [`taffy`]
//!
//! # Features
//!
//! - `std` (default): Enable standard library support
//! - `block` (default): Enable block layout support
//! - `flexbox` (default): Enable flexbox layout support
//! - `grid` (default): Enable CSS grid layout support
//! - `floats`: Enable float layout support
//! - `tracing`: Enable debug logging for unsupported CSS value fallbacks
//!
//! # Limitations
//!
//! This crate converts Stylo computed styles to Taffy layout styles. Some CSS features
//! are not yet supported by Taffy and will fall back to default values:
//!
//! - `min-content`, `max-content`, `fit-content()` sizing keywords fall back to `auto`
//! - `stretch` and `-webkit-fill-available` fall back to `auto`
//! - Anchor positioning functions fall back to `auto`
//! - CSS `position: fixed` and `position: sticky` are treated as `absolute` and `relative` respectively
//!
//! Enable the `tracing` feature to see debug logs when these fallbacks occur.
//!
//! # Safety
//!
//! The [`convert::length_percentage`](crate::convert::length_percentage) function uses `unsafe`
//! to convert calc() values. This is safe because:
//! - The pointer comes from Stylo's validated computed values
//! - Taffy's `from_raw` is designed to accept these specific pointer types

mod wrapper;
pub use wrapper::TaffyStyloStyle;

pub mod convert;
#[doc(inline)]
pub use convert::to_taffy_style;

pub use style::Atom;

#[cfg(test)]
mod tests;
