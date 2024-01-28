use std::{pin::Pin, rc::Rc, sync::Arc};

use concurrent_queue::ConcurrentQueue;
use derive_more::{Deref, DerefMut};
use event_listener::Event;
use futures_lite::{Future, FutureExt};
use mlua::prelude::*;

use crate::{handle::Handle, traits::IntoLuaThread, util::ThreadWithArgs};

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

    pub fn push_item_with_handle<'lua>(
        &self,
        lua: &'lua Lua,
        thread: impl IntoLuaThread<'lua>,
        args: impl IntoLuaMulti<'lua>,
    ) -> LuaResult<Handle> {
        let handle = Handle::new(lua, thread, args)?;
        let handle_thread = handle.create_thread(lua)?;

        self.push_item(lua, handle_thread, ())?;

        Ok(handle)
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
    Alias for [`ThreadQueue`], providing a newtype to store in Lua app data.
*/
#[derive(Debug, Clone, Deref, DerefMut)]
pub(crate) struct SpawnedThreadQueue(ThreadQueue);

impl SpawnedThreadQueue {
    pub fn new() -> Self {
        Self(ThreadQueue::new())
    }
}

/**
    Alias for [`ThreadQueue`], providing a newtype to store in Lua app data.
*/
#[derive(Debug, Clone, Deref, DerefMut)]
pub(crate) struct DeferredThreadQueue(ThreadQueue);

impl DeferredThreadQueue {
    pub fn new() -> Self {
        Self(ThreadQueue::new())
    }
}

pub type LocalBoxFuture<'fut> = Pin<Box<dyn Future<Output = ()> + 'fut>>;

/**
    Queue for storing local futures.

    Provides methods for pushing and draining the queue, as
    well as listening for new items being pushed to the queue.
*/
#[derive(Debug, Clone)]
pub(crate) struct FuturesQueue<'fut> {
    queue: Rc<ConcurrentQueue<LocalBoxFuture<'fut>>>,
    event: Arc<Event>,
}

impl<'fut> FuturesQueue<'fut> {
    pub fn new() -> Self {
        let queue = Rc::new(ConcurrentQueue::unbounded());
        let event = Arc::new(Event::new());
        Self { queue, event }
    }

    pub fn push_item(&self, fut: impl Future<Output = ()> + 'fut) {
        let _ = self.queue.push(fut.boxed_local());
        self.event.notify(usize::MAX);
    }

    pub fn drain_items<'outer>(
        &'outer self,
    ) -> impl Iterator<Item = LocalBoxFuture<'fut>> + 'outer {
        self.queue.try_iter()
    }

    pub async fn wait_for_item(&self) {
        if self.queue.is_empty() {
            self.event.listen().await;
        }
    }
}
