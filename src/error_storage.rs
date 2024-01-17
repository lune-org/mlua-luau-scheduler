use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use mlua::prelude::*;

#[derive(Debug, Clone)]
pub struct ErrorStorage {
    is_some: Arc<AtomicBool>,
    inner: Arc<Mutex<Option<LuaError>>>,
}

impl ErrorStorage {
    pub fn new() -> Self {
        Self {
            is_some: Arc::new(AtomicBool::new(false)),
            inner: Arc::new(Mutex::new(None)),
        }
    }

    #[inline]
    pub fn take(&self) -> Option<LuaError> {
        if self.is_some.load(Ordering::Relaxed) {
            self.inner.lock().unwrap().take()
        } else {
            None
        }
    }

    #[inline]
    pub fn replace(&self, e: LuaError) {
        self.is_some.store(true, Ordering::Relaxed);
        self.inner.lock().unwrap().replace(e);
    }
}
