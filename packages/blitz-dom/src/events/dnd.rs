use std::any::Any;

use blitz_traits::events::BlitzDataTransferItemsTrait;

#[derive(Debug, Clone)]
pub struct BlitzInternalDataTransferEntry {
    pub format: String,
    pub data: String,
}

#[derive(Debug, Default)]
pub struct BlitzInternalDataTransferItems {
    entries: Vec<BlitzInternalDataTransferEntry>,
    // todo when dragging images file is set
    _files: Vec<String>,
}

impl BlitzDataTransferItemsTrait for BlitzInternalDataTransferItems {
    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn get_data(&self, format: &str) -> Option<String> {
        self.entries
            .iter()
            .find(|entry| entry.format == format)
            .map(|entry| entry.data.clone())
    }

    fn set_data(&mut self, format: &str, data: &str) -> Result<(), String> {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.format == format) {
            entry.data = data.to_string();
        } else {
            self.entries.push(BlitzInternalDataTransferEntry {
                format: format.to_string(),
                data: data.to_string(),
            });
        }
        Ok(())
    }

    fn clear_all(&mut self) {
        self.entries.clear();
    }

    fn clear_format(&mut self, format: &str) {
        self.entries.retain(|entry| entry.format != format);
    }

    fn types(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|entry| entry.format.clone())
            .collect()
    }

    fn as_any(&self) -> &dyn Any {
        self as &dyn Any
    }
}
