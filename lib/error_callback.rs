use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use mlua::prelude::*;
use smol::lock::Mutex;

type ErrorCallback = Box<dyn Fn(LuaError) + Send + 'static>;

#[derive(Clone)]
pub(crate) struct ThreadErrorCallback {
    exists: Arc<AtomicBool>,
    inner: Arc<Mutex<Option<ErrorCallback>>>,
}

impl ThreadErrorCallback {
    pub fn new() -> Self {
        Self {
            exists: Arc::new(AtomicBool::new(false)),
            inner: Arc::new(Mutex::new(None)),
        }
    }

    pub fn new_default() -> Self {
        let this = Self::new();
        this.replace(default_error_callback);
        this
    }

    pub fn replace(&self, callback: impl Fn(LuaError) + Send + 'static) {
        self.exists.store(true, Ordering::Relaxed);
        self.inner.lock_blocking().replace(Box::new(callback));
    }

    pub fn clear(&self) {
        self.exists.store(false, Ordering::Relaxed);
        self.inner.lock_blocking().take();
    }

    pub fn call(&self, error: &LuaError) {
        if self.exists.load(Ordering::Relaxed) {
            if let Some(cb) = &*self.inner.lock_blocking() {
                cb(error.clone());
            }
        }
    }
}

fn default_error_callback(e: LuaError) {
    eprintln!("{e}");
}
