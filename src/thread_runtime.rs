use std::{cell::Cell, rc::Rc};

use mlua::prelude::*;
use smol::{
    channel::{Receiver, Sender},
    future::{yield_now, FutureExt},
    lock::Mutex,
    stream::StreamExt,
    *,
};

use super::{
    thread_util::{IntoLuaThread, LuaThreadOrFunction},
    ThreadWithArgs,
};

pub struct ThreadRuntime {
    queue_status: Rc<Cell<bool>>,
    queue_spawn: Rc<Mutex<Vec<ThreadWithArgs>>>,
    queue_defer: Rc<Mutex<Vec<ThreadWithArgs>>>,
    tx: Sender<()>,
    rx: Receiver<()>,
}

impl ThreadRuntime {
    /**
        Creates a new runtime for the given Lua state.

        This will inject some functions to interact with the scheduler / executor.
    */
    pub fn new(lua: &Lua) -> LuaResult<ThreadRuntime> {
        let queue_status = Rc::new(Cell::new(false));
        let queue_spawn = Rc::new(Mutex::new(Vec::new()));
        let queue_defer = Rc::new(Mutex::new(Vec::new()));
        let (tx, rx) = channel::unbounded();

        // HACK: Extract mlua "pending" constant value and store it
        let pending = lua
            .create_async_function(|_, ()| async move {
                yield_now().await;
                Ok(())
            })?
            .into_lua_thread(lua)?
            .resume::<_, LuaValue>(())?;
        let pending_key = lua.create_registry_value(pending)?;

        // Create spawn function (push to start of queue)
        let b_spawn = Rc::clone(&queue_status);
        let q_spawn = Rc::clone(&queue_spawn);
        let tx_spawn = tx.clone();
        let fn_spawn = lua.create_function(
            move |lua, (tof, args): (LuaThreadOrFunction, LuaMultiValue)| {
                let thread = tof.into_thread(lua)?;
                if thread.status() == LuaThreadStatus::Resumable {
                    // HACK: We need to resume the thread once instantly for correct behavior,
                    // and only if we get the pending value back we can spawn to async executor
                    let pending: LuaValue = lua.registry_value(&pending_key)?;
                    match thread.resume::<_, LuaValue>(args.clone()) {
                        Err(e) => {
                            eprintln!("{:?}", e);
                            // TODO: Forward error
                        }
                        Ok(v) if v == pending => {
                            let stored = ThreadWithArgs::new(lua, thread.clone(), args);
                            q_spawn.lock_blocking().push(stored);
                            b_spawn.replace(true);
                            tx_spawn.try_send(()).map_err(|_| {
                                LuaError::runtime("Tried to spawn thread to a dropped queue")
                            })?;
                        }
                        Ok(_) => {
                            // TODO: Forward value
                        }
                    }
                    Ok(thread)
                } else {
                    Err(LuaError::runtime("Tried to spawn non-resumable thread"))
                }
            },
        )?;

        // Create defer function (push to end of queue)
        let b_defer = Rc::clone(&queue_status);
        let q_defer = Rc::clone(&queue_defer);
        let tx_defer = tx.clone();
        let fn_defer = lua.create_function(
            move |lua, (tof, args): (LuaThreadOrFunction, LuaMultiValue)| {
                let thread = tof.into_thread(lua)?;
                if thread.status() == LuaThreadStatus::Resumable {
                    let stored = ThreadWithArgs::new(lua, thread.clone(), args);
                    q_defer.lock_blocking().push(stored);
                    b_defer.replace(true);
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

        Ok(ThreadRuntime {
            queue_status,
            queue_spawn,
            queue_defer,
            tx,
            rx,
        })
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

        self.queue_spawn.lock_blocking().push(stored);
        self.queue_status.replace(true);
        self.tx.try_send(()).unwrap(); // Unwrap is safe since this struct also holds the receiver
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
                // Wait for a new thread to arrive __or__ next futures step, prioritizing
                // new threads, so we don't accidentally exit when there is more work to do
                let fut_recv = async {
                    self.rx.recv().await.ok();
                };
                let fut_tick = async {
                    lua_exec.tick().await;
                    // Do as much work as possible
                    loop {
                        if !lua_exec.try_tick() {
                            break;
                        }
                    }
                };
                fut_recv.or(fut_tick).await;

                // If a new thread was spawned onto any queue, we
                // must drain them and schedule on the executor
                if self.queue_status.get() {
                    let mut queued_threads = Vec::new();
                    queued_threads.extend(self.queue_spawn.lock().await.drain(..));
                    queued_threads.extend(self.queue_defer.lock().await.drain(..));
                    for queued_thread in queued_threads {
                        // NOTE: Thread may have been cancelled from lua
                        // before we got here, so we need to check it again
                        let (thread, args) = queued_thread.into_inner(lua);
                        if thread.status() == LuaThreadStatus::Resumable {
                            let mut stream = thread.into_async::<_, LuaValue>(args);
                            lua_exec
                                .spawn(async move {
                                    // Only run stream until first coroutine.yield or completion. We will
                                    // drop it right away to clear stack space since detached tasks dont drop
                                    // until the executor drops https://github.com/smol-rs/smol/issues/294
                                    match stream.next().await.unwrap() {
                                        Err(e) => {
                                            eprintln!("{e}");
                                            // TODO: Forward error
                                        }
                                        Ok(_) => {
                                            // TODO: Forward value
                                        }
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
