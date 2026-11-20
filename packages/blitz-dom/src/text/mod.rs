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
//! Lengths across these traits are in CSS pixels, in and out. Device pixels (CSS pixels times
//! [`InlineText::scale`]) are only for paint-facing geometry: selection and caret rectangles, and
//! what blitz-paint reads from a backend's own types. A backend that works in device pixels scales
//! at its side of the boundary.
//!
//! Painting is the renderer's part: blitz-paint paints each backend's types in a module of its own,
//! which [`cfg_text_backend!`](crate::cfg_text_backend) selects.

#[cfg(not(any(feature = "parley", feature = "winkin")))]
compile_error!("Enable one of the `parley` or `winkin` features");

mod document;
mod editing;
mod inline;

pub use parlance;

pub(crate) use document::{DocumentText, TextContext};
pub use document::{FaceDescriptors, FontContext, TextFonts, build_single_font_ctx};
pub(crate) use editing::EditEngine;
pub use editing::{Edit, EditableText, EditorMetrics, Motion, TextEditor, TextInputDriver};
pub(crate) use inline::BackendLayout;
pub(crate) use inline::DeviceExclusions;
#[cfg_attr(feature = "floats", allow(unused_imports))]
pub(crate) use inline::NoLineExclusions;
pub(crate) use inline::{
    BoxMeasure, ContentWidths, DocumentCascade, FloatClear, FloatSide, InlineBoxKind,
    InlineBuilder, InlineLayoutEngine, LineArea, LineExclusions, LineFlow, LinesExtent,
    LinesInputs, MarkerEngine, Measure, Placement, SpanKind,
};
#[cfg_attr(not(feature = "floats"), allow(unused_imports))]
pub(crate) use inline::{FloatRequest, LineSlot, PlacedFloat, SlotSearch};
pub use inline::{InlineText, MarkerLayout, TextLayout};

#[cfg(feature = "parley")]
pub mod parley;
#[cfg(all(feature = "winkin", not(feature = "parley")))]
pub mod winkin;

#[cfg(feature = "parley")]
use self::parley as backend;
#[cfg(all(feature = "winkin", not(feature = "parley")))]
use self::winkin as backend;

/// The text backends Blitz can be built with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextBackend {
    /// [Parley](https://github.com/linebender/parley), the `parley` feature.
    Parley,
    /// [winkin](https://github.com/dfrg/winkin), the `winkin` feature.
    Winkin,
}

/// The text backend this build of Blitz lays out text with.
#[cfg(feature = "parley")]
pub const BACKEND: TextBackend = TextBackend::Parley;
/// The text backend this build of Blitz lays out text with.
#[cfg(all(feature = "winkin", not(feature = "parley")))]
pub const BACKEND: TextBackend = TextBackend::Winkin;

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
///     winkin => { mod winkin; use winkin as backend; }
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

/// Expands the arm of [`cfg_text_backend!`] named for the text backend in use.
#[cfg(all(feature = "winkin", not(feature = "parley")))]
#[doc(hidden)]
#[macro_export]
macro_rules! __text_backend_arms {
    () => {};
    (winkin => { $($tokens:tt)* } $($rest:tt)*) => {
        $($tokens)*
        $crate::__text_backend_arms! { $($rest)* }
    };
    ($other:ident => { $($tokens:tt)* } $($rest:tt)*) => {
        $crate::__text_backend_arms! { $($rest)* }
    };
}

#[doc(hidden)]
pub static PERF_COUNTS: [core::sync::atomic::AtomicUsize; 2] =
    [const { core::sync::atomic::AtomicUsize::new(0) }; 2];

#[cfg(test)]
mod tests {
    use super::{BACKEND, TextBackend};

    /// Parley is the backend wherever its feature is on, winkin's or not.
    #[test]
    fn parley_wins_when_both_backends_are_enabled() {
        let expected = if cfg!(feature = "parley") {
            TextBackend::Parley
        } else {
            TextBackend::Winkin
        };
        assert_eq!(BACKEND, expected);
    }
}
