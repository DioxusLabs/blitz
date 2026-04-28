use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Default)]
pub struct ConfigStore {
    inner: RwLock<HashMap<String, String>>,
}

impl ConfigStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.inner.read().ok()?.get(key).cloned()
    }

    pub fn set(&self, key: &str, value: &str) {
        if let Ok(mut map) = self.inner.write() {
            map.insert(key.to_string(), value.to_string());
        }
    }
}
