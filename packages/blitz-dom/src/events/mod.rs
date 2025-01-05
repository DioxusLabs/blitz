mod ime;
mod keyboard;
mod mouse;

pub(crate) use ime::handle_ime_event;
pub(crate) use keyboard::handle_keypress;
pub(crate) use mouse::handle_click;

use crate::Document;
use winit::event::{Ime, KeyEvent, Modifiers};

pub(crate) fn handle_event(doc: &mut Document, event: DomEvent) {
    let target_node_id = event.target;

    match event.data {
        DomEventData::MouseDown { .. } | DomEventData::MouseUp { .. } => {}
        DomEventData::Hover => {}
        DomEventData::Click { x, y, .. } => {
            handle_click(doc, target_node_id, x, y);
        }
        DomEventData::KeyPress { event, mods } => {
            handle_keypress(doc, target_node_id, event, mods);
        }
        DomEventData::Ime(ime_event) => {
            handle_ime_event(doc, ime_event);
        }
    }
}

pub struct EventListener {
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct DomEvent {
    pub target: usize,
    pub data: DomEventData,
}

impl DomEvent {
    /// Returns the name of the event ("click", "mouseover", "keypress", etc)
    pub fn name(&self) -> &'static str {
        self.data.name()
    }
}

#[derive(Debug, Clone)]
pub enum DomEventData {
    MouseDown { x: f32, y: f32, mods: Modifiers },
    MouseUp { x: f32, y: f32, mods: Modifiers },
    Click { x: f32, y: f32, mods: Modifiers },
    KeyPress { event: KeyEvent, mods: Modifiers },
    Ime(Ime),
    Hover,
}

impl DomEventData {
    pub fn name(&self) -> &'static str {
        match self {
            DomEventData::MouseDown { .. } => "mousedown",
            DomEventData::MouseUp { .. } => "mouseup",
            DomEventData::Click { .. } => "click",
            DomEventData::KeyPress { .. } => "keypress",
            DomEventData::Ime { .. } => "input",
            DomEventData::Hover => "mouseover",
        }
    }
}

#[derive(Debug, Clone)]
pub struct HitResult {
    /// The node_id of the node identified as the hit target
    pub node_id: usize,
    /// The x coordinate of the hit within the hit target's border-box
    pub x: f32,
    /// The y coordinate of the hit within the hit target's border-box
    pub y: f32,
}
