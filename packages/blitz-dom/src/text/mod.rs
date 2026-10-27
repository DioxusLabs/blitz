//! Text layout, text editing and fonts, behind one interface for every text backend.
//!
//! Blitz lays out text with one backend, chosen when it is built. Each backend is a module here
//! that implements the traits below for its own types, and the type aliases name the types of the
//! backend in use, so the rest of Blitz is written once, against the aliases and the traits.
//!
//! - [`FontContext`] ([`TextFonts`]): the fonts documents lay out text with.
//! - [`TextLayout`] ([`InlineText`]): an inline formatting context, built from the DOM, measured,
//!   broken into lines and placed, and read back for hit testing, selection and `innerText`.
//! - [`TextEditor`] ([`EditableText`]): the text of an `<input>` or `<textarea>`, and its editing.
//! - [`MarkerLayout`]: an outside list marker.
//! - [`FaceDescriptors`]: an `@font-face` rule's descriptors, which a backend registers a face by,
//!   in [`parlance`]'s font value types.
//!
//! Painting is the renderer's part: blitz-paint paints each backend's types in a module of its own,
//! which [`cfg_text_backend!`](crate::cfg_text_backend) selects.

#[cfg(not(feature = "parley"))]
compile_error!("Enable the `parley` feature: Blitz needs a text backend");

mod document;
mod editing;
mod inline;

pub use parlance;

pub(crate) use document::{DocumentText, TextContext};
pub use document::{FaceDescriptors, FontContext, TextFonts, build_single_font_ctx};
pub(crate) use editing::EditEngine;
pub use editing::{Edit, EditableText, EditorMetrics, Motion, TextEditor, TextInputDriver};
#[cfg_attr(feature = "floats", allow(unused_imports))]
pub(crate) use inline::NoLineExclusions;
pub(crate) use inline::{
    BoxMeasure, ContentWidths, FloatClear, FloatSide, InlineBoxKind, InlineBuilder,
    InlineLayoutEngine, LineExclusions, LinesExtent, MarkerEngine, Placement, SpanKind,
};
#[cfg_attr(not(feature = "floats"), allow(unused_imports))]
pub(crate) use inline::{FloatRequest, LineSlot, PlacedFloat};
pub use inline::{InlineText, MarkerLayout, TextLayout};

#[cfg(feature = "parley")]
pub mod parley;

#[cfg(feature = "parley")]
use self::parley as backend;

/// The text backends Blitz can be built with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextBackend {
    /// [Parley](https://github.com/linebender/parley), the `parley` feature.
    Parley,
}

/// The text backend this build of Blitz lays out text with.
#[cfg(feature = "parley")]
pub const BACKEND: TextBackend = TextBackend::Parley;

/// Expands the arm for the text backend in use, and drops the others, so that a crate which
/// holds code for each backend compiles the one blitz-dom was built with.
///
/// Each arm is a backend's name and its tokens in braces. The arms of backends other than the one
/// in use are dropped, whether or not this build knows them, so callers can list every backend
/// they hold code for.
///
/// ```ignore
/// blitz_dom::cfg_text_backend! {
///     parley => { mod parley; use parley as backend; }
/// }
/// ```
#[macro_export]
macro_rules! cfg_text_backend {
    ($($arms:tt)*) => {
        $crate::__text_backend_arms! { $($arms)* }
    };
}

/// Expands the arm of [`cfg_text_backend!`] named for the text backend in use.
#[cfg(feature = "parley")]
#[doc(hidden)]
#[macro_export]
macro_rules! __text_backend_arms {
    () => {};
    (parley => { $($tokens:tt)* } $($rest:tt)*) => {
        $($tokens)*
        $crate::__text_backend_arms! { $($rest)* }
    };
    ($other:ident => { $($tokens:tt)* } $($rest:tt)*) => {
        $crate::__text_backend_arms! { $($rest)* }
    };
}

#[cfg(test)]
mod tests {
    use super::{BACKEND, TextBackend};

    /// The `parley` feature selects Parley.
    #[test]
    fn parley_is_the_backend() {
        assert_eq!(BACKEND, TextBackend::Parley);
    }
}
