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
