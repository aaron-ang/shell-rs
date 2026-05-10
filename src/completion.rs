use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

#[derive(Clone)]
pub struct Completions {
    inner: Arc<RwLock<HashMap<String, String>>>,
}

impl Completions {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn register(&self, command: String, script: String) {
        self.inner.write().unwrap().insert(command, script);
    }

    pub fn get(&self, command: &str) -> Option<String> {
        self.inner.read().unwrap().get(command).cloned()
    }

    pub fn remove(&self, command: &str) {
        self.inner.write().unwrap().remove(command);
    }
}
