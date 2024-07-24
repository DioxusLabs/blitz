use winit::{event::KeyEvent, event::Modifiers};

pub struct EventListener {
    pub name: String,
}

#[derive(Debug)]
pub struct RendererEvent {
    pub target: usize,
    pub data: EventData,
}

impl RendererEvent {
    /// Returns the name of the event ("click", "mouseover", "keypress", etc)
    pub fn name(&self) -> &'static str {
        self.data.name()
    }
}

#[derive(Debug)]
pub enum EventData {
    Click { x: f32, y: f32, mods: Modifiers },
    KeyPress { event: KeyEvent, mods: Modifiers },
    Hover,
}

impl EventData {
    pub fn name(&self) -> &'static str {
        match self {
            EventData::Click { .. } => "click",
            EventData::KeyPress { .. } => "keypress",
            EventData::Hover => "mouseover",
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
