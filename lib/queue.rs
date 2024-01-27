use std::sync::Arc;

use concurrent_queue::ConcurrentQueue;
use event_listener::Event;
use mlua::prelude::*;

use crate::{util::ThreadWithArgs, IntoLuaThread};

/**
    Queue for storing [`LuaThread`]s with associated arguments.

    Provides methods for pushing and draining the queue, as
    well as listening for new items being pushed to the queue.
*/
#[derive(Debug, Clone)]
pub(crate) struct ThreadQueue {
    queue: Arc<ConcurrentQueue<ThreadWithArgs>>,
    event: Arc<Event>,
}

impl ThreadQueue {
    pub fn new() -> Self {
        let queue = Arc::new(ConcurrentQueue::unbounded());
        let event = Arc::new(Event::new());
        Self { queue, event }
    }

    pub fn push_item<'lua>(
        &self,
        lua: &'lua Lua,
        thread: impl IntoLuaThread<'lua>,
        args: impl IntoLuaMulti<'lua>,
    ) -> LuaResult<()> {
        let thread = thread.into_lua_thread(lua)?;
        let args = args.into_lua_multi(lua)?;

        tracing::trace!("pushing item to queue with {} args", args.len());
        let stored = ThreadWithArgs::new(lua, thread, args)?;

        self.queue.push(stored).into_lua_err()?;
        self.event.notify(usize::MAX);

        Ok(())
    }

    pub fn drain_items<'outer, 'lua>(
        &'outer self,
        lua: &'lua Lua,
    ) -> impl Iterator<Item = (LuaThread<'lua>, LuaMultiValue<'lua>)> + 'outer
    where
        'lua: 'outer,
    {
        self.queue.try_iter().map(|stored| stored.into_inner(lua))
    }

    pub async fn wait_for_item(&self) {
        if self.queue.is_empty() {
            self.event.listen().await;
        }
    }
}
