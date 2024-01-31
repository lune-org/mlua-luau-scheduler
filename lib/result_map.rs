#![allow(clippy::inline_always)]

use std::{cell::RefCell, rc::Rc};

// NOTE: This is the hash algorithm that mlua also uses, so we
// are not adding any additional dependencies / bloat by using it.
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{thread_id::ThreadId, util::ThreadResult};

#[derive(Clone)]
pub(crate) struct ThreadResultMap {
    tracked: Rc<RefCell<FxHashSet<ThreadId>>>,
    inner: Rc<RefCell<FxHashMap<ThreadId, ThreadResult>>>,
}

impl ThreadResultMap {
    pub fn new() -> Self {
        Self {
            tracked: Rc::new(RefCell::new(FxHashSet::default())),
            inner: Rc::new(RefCell::new(FxHashMap::default())),
        }
    }

    #[inline(always)]
    pub fn track(&self, id: ThreadId) {
        self.tracked.borrow_mut().insert(id);
    }

    #[inline(always)]
    pub fn is_tracked(&self, id: ThreadId) -> bool {
        self.tracked.borrow().contains(&id)
    }

    #[inline(always)]
    pub fn insert(&self, id: ThreadId, result: ThreadResult) {
        debug_assert!(self.is_tracked(id), "Thread must be tracked");
        self.inner.borrow_mut().insert(id, result);
    }

    pub fn remove(&self, id: ThreadId) -> Option<ThreadResult> {
        let res = self.inner.borrow_mut().remove(&id)?;
        self.tracked.borrow_mut().remove(&id);
        Some(res)
    }
}
