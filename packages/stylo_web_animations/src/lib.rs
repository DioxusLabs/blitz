//! An implementation of the [Web Animations](https://drafts.csswg.org/web-animations-1/) model
//! that does not depend on a DOM or a script engine.
//!
//! This crate is an implementation detail of [`blitz-dom`](https://docs.rs/blitz-dom), but can
//! also be used standalone.

mod animation;
mod effect;
mod store;
pub use animation::{Action, Animation, Error, EventKind, PlayState};
pub use effect::KeyframeEffect;
pub use store::{
    AnimationId, AnimationStore, ComposedValues, CssAnimation, CssEvent, CssEventKind, Origin,
    ReplaceState, Target,
};
