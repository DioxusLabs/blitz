//! A native renderer for HTML/CSS.
//!
//! ## Feature flags
//!  - `default`: Enables the features listed below.
//!  - `tracing`: Enables tracing support.

/*

lib.rs -> DioxusNative -> Window -> Document

*/

mod devtools;
pub mod renderer;
mod util;

pub use devtools::*;
pub use renderer::*;
