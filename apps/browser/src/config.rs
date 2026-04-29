use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

type Listener = Box<dyn Fn(&str, &str) + Send + Sync>;

#[derive(Default)]
pub struct ConfigStore {
    inner: RwLock<HashMap<String, String>>,
    listeners: Mutex<Vec<Listener>>,
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
        if let Ok(listeners) = self.listeners.lock() {
            for listener in listeners.iter() {
                listener(key, value);
            }
        }
    }

    pub fn subscribe<F>(&self, listener: F)
    where
        F: Fn(&str, &str) + Send + Sync + 'static,
    {
        if let Ok(mut listeners) = self.listeners.lock() {
            listeners.push(Box::new(listener));
        }
    }
}
