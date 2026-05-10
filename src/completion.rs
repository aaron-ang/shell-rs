use std::{cell::RefCell, collections::HashMap, rc::Rc};

#[derive(Clone)]
pub struct Completions {
    inner: Rc<RefCell<HashMap<String, String>>>,
}

impl Completions {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    pub fn register(&self, command: String, script: String) {
        self.inner.borrow_mut().insert(command, script);
    }

    pub fn get(&self, command: &str) -> Option<String> {
        self.inner.borrow().get(command).cloned()
    }

    pub fn remove(&self, command: &str) {
        self.inner.borrow_mut().remove(command);
    }
}
