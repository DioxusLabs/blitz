pub struct EventListener {
    pub name: String,
}

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

pub enum EventData {
    Click { x: f64, y: f64 },
    Hover,
}

impl EventData {
    pub fn name(&self) -> &'static str {
        match self {
            EventData::Click { .. } => "click",
            EventData::Hover => "mouseover",
        }
    }
}

pub struct HitResult {
    /// The node_id of the node identified as the hit target
    pub node_id: usize,
    /// The x coordinate of the hit within the hit target's border-box
    pub x: f32,
    /// The y coordinate of the hit within the hit target's border-box
    pub y: f32,
}
