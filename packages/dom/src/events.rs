pub struct EventListener {
    pub name: String,
}

pub struct RendererEvent {
    pub name: String,
    pub target: usize,
    pub data: EventData,
}

pub enum EventData {
    Click { x: f64, y: f64 },
    Hover,
}

pub struct HitResult {
    /// The node_id of the node identified as the hit target
    pub node_id: usize,
    /// The x coordinate of the hit within the hit target's border-box
    pub x: f32,
    /// The y coordinate of the hit within the hit target's border-box
    pub y: f32,
}
