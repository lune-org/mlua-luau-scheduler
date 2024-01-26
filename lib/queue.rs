use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use mlua::prelude::*;
use smol::{
    channel::{unbounded, Receiver, Sender},
    lock::Mutex,
};

use crate::IntoLuaThread;

const ERR_OOM: &str = "out of memory";

/**
    Queue for storing [`LuaThread`]s with associated arguments.

    Provides methods for pushing and draining the queue, as
    well as listening for new items being pushed to the queue.
*/
#[derive(Debug, Clone)]
pub struct ThreadQueue {
    queue: Arc<Mutex<Vec<ThreadWithArgs>>>,
    status: Arc<AtomicBool>,
    signal_tx: Sender<()>,
    signal_rx: Receiver<()>,
}

impl ThreadQueue {
    pub fn new() -> Self {
        let (signal_tx, signal_rx) = unbounded();
        Self {
            queue: Arc::new(Mutex::new(Vec::new())),
            status: Arc::new(AtomicBool::new(false)),
            signal_tx,
            signal_rx,
        }
    }

    pub fn has_threads(&self) -> bool {
        self.status.load(Ordering::SeqCst)
    }

    pub fn push<'lua>(
        &self,
        lua: &'lua Lua,
        thread: impl IntoLuaThread<'lua>,
        args: impl IntoLuaMulti<'lua>,
    ) -> LuaResult<()> {
        let thread = thread.into_lua_thread(lua)?;
        let args = args.into_lua_multi(lua)?;
        let stored = ThreadWithArgs::new(lua, thread, args);

        self.queue.lock_blocking().push(stored);
        self.status.store(true, Ordering::SeqCst);
        self.signal_tx.try_send(()).unwrap();

        Ok(())
    }

    pub async fn drain<'lua>(&self, lua: &'lua Lua) -> Vec<(LuaThread<'lua>, LuaMultiValue<'lua>)> {
        let mut queue = self.queue.lock().await;
        let drained = queue.drain(..).map(|s| s.into_inner(lua)).collect();
        self.status.store(false, Ordering::SeqCst);
        drained
    }

    pub async fn recv(&self) {
        self.signal_rx.recv().await.unwrap();
        // Drain any pending receives
        loop {
            match self.signal_rx.try_recv() {
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    }
}

/**
    Representation of a [`LuaThread`] with associated arguments currently stored in the Lua registry.
*/
#[derive(Debug)]
struct ThreadWithArgs {
    key_thread: LuaRegistryKey,
    key_args: LuaRegistryKey,
}

impl ThreadWithArgs {
    pub fn new<'lua>(lua: &'lua Lua, thread: LuaThread<'lua>, args: LuaMultiValue<'lua>) -> Self {
        let argsv = args.into_vec();

        let key_thread = lua.create_registry_value(thread).expect(ERR_OOM);
        let key_args = lua.create_registry_value(argsv).expect(ERR_OOM);

        Self {
            key_thread,
            key_args,
        }
    }

    pub fn into_inner(self, lua: &Lua) -> (LuaThread<'_>, LuaMultiValue<'_>) {
        let thread = lua.registry_value(&self.key_thread).unwrap();
        let argsv = lua.registry_value(&self.key_args).unwrap();

        let args = LuaMultiValue::from_vec(argsv);

        lua.remove_registry_value(self.key_thread).unwrap();
        lua.remove_registry_value(self.key_args).unwrap();

        (thread, args)
    }
}
