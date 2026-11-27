//! A code editing control built on [floem-editor-core](floem_editor_core) (editing model),
//! [Parley](parley) (text layout) and [AnyRender](anyrender) (painting).
//!
//! The editor doesn't depend on a windowing system or a particular renderer. Hosts feed it
//! [`EditorInput`]s, apply the [`EditorResponse`] it returns (redraws, clipboard and IME
//! requests), and call [`CodeEditor::paint`] with any [`anyrender::PaintScene`].

mod editor;
mod highlight;
mod input;
mod layout;
mod paint;
mod style;
mod text;

pub use editor::{CodeEditor, EditorResponse};
pub use floem_editor_core::xi_rope::Rope;
#[cfg(feature = "syntaxmate")]
pub use highlight::SyntaxmateHighlighter;
pub use highlight::{SpanStyle, StyledSpan, SyntaxHighlighter};
pub use input::{EditorInput, ImeInput, Key, KeyInput, Modifiers, PointerButton, ScrollDelta};
pub use style::{EditorStyle, Theme};
pub use text::{TextBrush, TextContext};
