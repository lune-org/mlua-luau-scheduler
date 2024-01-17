use std::{sync::Arc, time::Duration};

use dashmap::DashMap;
use tokio::time::Instant;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum StatsCounter {
    ThreadResumed,
    ThreadCancelled,
    ThreadSlept,
    ThreadErrored,
    WriteStdout,
    WriteStderr,
}

#[derive(Debug, Clone)]
pub struct Stats {
    start: Instant,
    pub counters: Arc<DashMap<StatsCounter, usize>>,
}

impl Stats {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            counters: Arc::new(DashMap::new()),
        }
    }

    pub fn incr(&self, counter: StatsCounter) {
        self.counters
            .entry(counter)
            .and_modify(|c| *c += 1)
            .or_insert(1);
    }

    pub fn elapsed(&self) -> Duration {
        Instant::now() - self.start
    }
}
