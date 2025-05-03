use crate::DomEvent;
use std::any::Any;

pub trait Document: AsRef<Self::Doc> + AsMut<Self::Doc> + Into<Self::Doc> + 'static {
    type Doc: 'static;

    fn poll(&mut self, _cx: std::task::Context) -> bool {
        // Default implementation does nothing
        false
    }

    fn handle_event(&mut self, _event: &mut DomEvent) {
        // Default implementation does nothing
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn id(&self) -> usize;
}
