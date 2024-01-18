use std::{collections::VecDeque, rc::Rc};

use mlua::prelude::*;
use smol::{
    channel::{Receiver, Sender},
    future::race,
    lock::Mutex,
    *,
};

use super::{
    thread_util::{IntoLuaThread, LuaThreadOrFunction},
    ThreadWithArgs,
};

pub struct ThreadRuntime {
    queue: Rc<Mutex<VecDeque<ThreadWithArgs>>>,
    tx: Sender<()>,
    rx: Receiver<()>,
}

impl ThreadRuntime {
    /**
        Creates a new runtime for the given Lua state.

        This will inject some functions to interact with the scheduler / executor.
    */
    pub fn new(lua: &Lua) -> LuaResult<ThreadRuntime> {
        let queue = Rc::new(Mutex::new(VecDeque::new()));
        let (tx, rx) = channel::unbounded();

        // Create spawn function (push to start of queue)
        let queue_spawn = Rc::clone(&queue);
        let tx_spawn = tx.clone();
        let fn_spawn = lua.create_function(
            move |lua, (tof, args): (LuaThreadOrFunction, LuaMultiValue)| {
                let thread = tof.into_thread(lua)?;
                if thread.status() == LuaThreadStatus::Resumable {
                    let stored = ThreadWithArgs::new(lua, thread.clone(), args);
                    queue_spawn.lock_blocking().push_front(stored);
                    tx_spawn.try_send(()).map_err(|_| {
                        LuaError::runtime("Tried to spawn thread to a dropped queue")
                    })?;
                    Ok(thread)
                } else {
                    Err(LuaError::runtime("Tried to spawn non-resumable thread"))
                }
            },
        )?;

        // Create defer function (push to end of queue)
        let queue_defer = Rc::clone(&queue);
        let tx_defer = tx.clone();
        let fn_defer = lua.create_function(
            move |lua, (tof, args): (LuaThreadOrFunction, LuaMultiValue)| {
                let thread = tof.into_thread(lua)?;
                if thread.status() == LuaThreadStatus::Resumable {
                    let stored = ThreadWithArgs::new(lua, thread.clone(), args);
                    queue_defer.lock_blocking().push_back(stored);
                    tx_defer.try_send(()).map_err(|_| {
                        LuaError::runtime("Tried to defer thread to a dropped queue")
                    })?;
                    Ok(thread)
                } else {
                    Err(LuaError::runtime("Tried to defer non-resumable thread"))
                }
            },
        )?;

        // FUTURE: Store these as named registry values instead
        // so that they are not accessible from within user code
        lua.globals().set("spawn", fn_spawn)?;
        lua.globals().set("defer", fn_defer)?;

        Ok(ThreadRuntime { queue, tx, rx })
    }

    /**
        Pushes a chunk / function / thread to the front of the runtime.
    */
    pub fn push_main<'lua>(
        &self,
        lua: &'lua Lua,
        thread: impl IntoLuaThread<'lua>,
        args: impl IntoLuaMulti<'lua>,
    ) {
        let thread = thread
            .into_lua_thread(lua)
            .expect("failed to create thread");
        let args = args.into_lua_multi(lua).expect("failed to create args");

        let stored = ThreadWithArgs::new(lua, thread, args);

        self.queue.lock_blocking().push_front(stored);
        self.tx.try_send(()).unwrap();
    }

    /**
        Runs the runtime until all Lua threads have completed.

        Note that the given Lua state must be the same one that was
        used to create this runtime, otherwise this method may panic.
    */
    pub async fn run_async(&self, lua: &Lua) {
        // Create new executors to use
        let lua_exec = LocalExecutor::new();
        let main_exec = Executor::new();

        // Tick local lua executor while also driving main
        // executor forward, until all lua threads finish
        let fut = async {
            loop {
                let did_spawn = race(
                    // Wait for next futures step...
                    async {
                        lua_exec.tick().await;
                        false
                    },
                    // ...or for a new thread to arrive
                    async {
                        self.rx.recv().await.ok();
                        true
                    },
                )
                .await;

                // If a new thread was spawned onto queue, we
                // must drain it and schedule on the executor
                if did_spawn {
                    let queued_threads = self.queue.lock().await.drain(..).collect::<Vec<_>>();
                    for queued_thread in queued_threads {
                        // NOTE: Thread may have been cancelled from lua
                        // before we got here, so we need to check it again
                        let (thread, args) = queued_thread.into_inner(lua);
                        if thread.status() == LuaThreadStatus::Resumable {
                            let fut = thread.into_async::<_, ()>(args);
                            lua_exec
                                .spawn(async move {
                                    match fut.await {
                                        Ok(()) => {}
                                        Err(e) => eprintln!("{e}"),
                                    }
                                })
                                .detach();
                        }
                    }
                }

                // Empty executor = no remaining threads
                if lua_exec.is_empty() {
                    break;
                }
            }
        };

        main_exec.run(fut).await
    }

    /**
        Runs the runtime until all Lua threads have completed, blocking the thread.

        See [`ThreadRuntime::run_async`] for more info.
    */
    pub fn run_blocking(&self, lua: &Lua) {
        block_on(self.run_async(lua))
    }
}