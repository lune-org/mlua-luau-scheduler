use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use crate::{thread_id::ThreadId, util::ThreadResult};

#[derive(Clone)]
pub(crate) struct ThreadResultMap {
    tracked: Rc<RefCell<HashSet<ThreadId>>>,
    inner: Rc<RefCell<HashMap<ThreadId, ThreadResult>>>,
}

impl ThreadResultMap {
    pub fn new() -> Self {
        Self {
            tracked: Rc::new(RefCell::new(HashSet::new())),
            inner: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    pub fn track(&self, id: ThreadId) {
        self.tracked.borrow_mut().insert(id);
    }

    pub fn is_tracked(&self, id: ThreadId) -> bool {
        self.tracked.borrow().contains(&id)
    }

    pub fn insert(&self, id: ThreadId, result: ThreadResult) {
        assert!(self.is_tracked(id), "Thread must be tracked");
        self.inner.borrow_mut().insert(id, result);
    }

    pub fn remove(&self, id: ThreadId) -> Option<ThreadResult> {
        let res = self.inner.borrow_mut().remove(&id)?;
        self.tracked.borrow_mut().remove(&id);
        Some(res)
    }
}
