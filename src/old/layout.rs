use std::sync::{Arc, Mutex};

// use dioxus_native_core::layout_attributes::apply_layout_attributes;
// use dioxus_native_core::prelude::*;
// use dioxus_native_core_macro::partial_derive_state;
// use shipyard::Component;
use taffy::prelude::*;

// use crate::image::LoadedImage;
// use crate::text::{FontSize, TextContext};

// TODO: More layout types. This should default to box layout
#[derive(Clone, Default, Debug)]
pub(crate) struct TaffyLayout {
    pub style: Style,
    pub node: Option<Node>,
}
