use std::sync::Arc;

use concurrent_queue::ConcurrentQueue;
use event_listener::Event;
use mlua::prelude::*;

use crate::IntoLuaThread;

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

/**
    Representation of a [`LuaThread`] with its associated arguments currently stored in the Lua registry.
*/
#[derive(Debug)]
struct ThreadWithArgs {
    key_thread: LuaRegistryKey,
    key_args: LuaRegistryKey,
}

impl ThreadWithArgs {
    fn new<'lua>(
        lua: &'lua Lua,
        thread: LuaThread<'lua>,
        args: LuaMultiValue<'lua>,
    ) -> LuaResult<Self> {
        let argsv = args.into_vec();

        let key_thread = lua.create_registry_value(thread)?;
        let key_args = lua.create_registry_value(argsv)?;

        Ok(Self {
            key_thread,
            key_args,
        })
    }

    fn into_inner(self, lua: &Lua) -> (LuaThread<'_>, LuaMultiValue<'_>) {
        let thread = lua.registry_value(&self.key_thread).unwrap();
        let argsv = lua.registry_value(&self.key_args).unwrap();

        let args = LuaMultiValue::from_vec(argsv);

        lua.remove_registry_value(self.key_thread).unwrap();
        lua.remove_registry_value(self.key_args).unwrap();

        (thread, args)
    }
}
