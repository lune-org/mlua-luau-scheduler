use std::sync::{Arc, Weak};
use std::time::Duration;

use mlua::prelude::*;
use smol::{prelude::*, Timer};

use smol::{block_on, Executor, LocalExecutor};

use super::{
    error_callback::ThreadErrorCallback, queue::ThreadQueue, traits::IntoLuaThread,
    util::LuaThreadOrFunction,
};

pub struct Runtime<'lua> {
    lua: &'lua Lua,
    queue_spawn: ThreadQueue,
    queue_defer: ThreadQueue,
    error_callback: ThreadErrorCallback,
}

impl<'lua> Runtime<'lua> {
    /**
        Creates a new runtime for the given Lua state.

        This runtime will have a default error callback that prints errors to stderr.
    */
    pub fn new(lua: &'lua Lua) -> LuaResult<Runtime<'lua>> {
        let queue_spawn = ThreadQueue::new();
        let queue_defer = ThreadQueue::new();
        let error_callback = ThreadErrorCallback::default();

        Ok(Runtime {
            lua,
            queue_spawn,
            queue_defer,
            error_callback,
        })
    }

    /**
        Sets the error callback for this runtime.

        This callback will be called whenever a Lua thread errors.

        Overwrites any previous error callback.
    */
    pub fn set_error_callback(&self, callback: impl Fn(LuaError) + Send + 'static) {
        self.error_callback.replace(callback);
    }

    /**
        Clears the error callback for this runtime.

        This will remove any current error callback, including default(s).
    */
    pub fn remove_error_callback(&self) {
        self.error_callback.clear();
    }

    /**
        Spawns a chunk / function / thread onto the runtime queue.

        Threads are guaranteed to be resumed in the order that they were pushed to the queue.
    */
    pub fn spawn_thread(
        &self,
        thread: impl IntoLuaThread<'lua>,
        args: impl IntoLuaMulti<'lua>,
    ) -> LuaResult<()> {
        let thread = thread.into_lua_thread(self.lua)?;
        let args = args.into_lua_multi(self.lua)?;

        self.queue_spawn.push(self.lua, thread, args)?;

        Ok(())
    }

    /**
        Defers a chunk / function / thread onto the runtime queue.

        Deferred threads are guaranteed to run after all spawned threads either yield or complete.

        Threads are guaranteed to be resumed in the order that they were pushed to the queue.
    */
    pub fn defer_thread(
        &self,
        thread: impl IntoLuaThread<'lua>,
        args: impl IntoLuaMulti<'lua>,
    ) -> LuaResult<()> {
        let thread = thread.into_lua_thread(self.lua)?;
        let args = args.into_lua_multi(self.lua)?;

        self.queue_defer.push(self.lua, thread, args)?;

        Ok(())
    }

    /**
        Creates a lua function that can be used to spawn threads / functions onto the runtime queue.

        The function takes a thread or function as the first argument, and any variadic arguments as the rest.
    */
    pub fn create_spawn_function(&self) -> LuaResult<LuaFunction<'lua>> {
        let error_callback = self.error_callback.clone();
        let spawn_queue = self.queue_spawn.clone();
        self.lua.create_function(
            move |lua, (tof, args): (LuaThreadOrFunction, LuaMultiValue)| {
                let thread = tof.into_thread(lua)?;
                if thread.status() == LuaThreadStatus::Resumable {
                    // NOTE: We need to resume the thread once instantly for correct behavior,
                    // and only if we get the pending value back we can spawn to async executor
                    match thread.resume::<_, LuaValue>(args.clone()) {
                        Ok(v) => {
                            if v.as_light_userdata()
                                .map(|l| l == Lua::poll_pending())
                                .unwrap_or_default()
                            {
                                spawn_queue.push(lua, &thread, args)?;
                            }
                        }
                        Err(e) => {
                            error_callback.call(&e);
                        }
                    };
                }
                Ok(thread)
            },
        )
    }

    /**
        Creates a lua function that can be used to defer threads / functions onto the runtime queue.

        The function takes a thread or function as the first argument, and any variadic arguments as the rest.

        Deferred threads are guaranteed to run after all spawned threads either yield or complete.
    */
    pub fn create_defer_function(&self) -> LuaResult<LuaFunction<'lua>> {
        let defer_queue = self.queue_defer.clone();
        self.lua.create_function(
            move |lua, (tof, args): (LuaThreadOrFunction, LuaMultiValue)| {
                let thread = tof.into_thread(lua)?;
                if thread.status() == LuaThreadStatus::Resumable {
                    defer_queue.push(lua, &thread, args)?;
                }
                Ok(thread)
            },
        )
    }

    /**
        Runs the runtime until all Lua threads have completed.

        Note that the given Lua state must be the same one that was
        used to create this runtime, otherwise this method may panic.
    */
    pub async fn run_async(&self) {
        // Make sure we do not already have an executor - this is a definite user error
        // and may happen if the user tries to run multiple runtimes on the same lua state
        if self.lua.app_data_ref::<Weak<Executor>>().is_some() {
            panic!(
                "Lua state already has an executor attached!\
                \nOnly one runtime can be used per lua state."
            );
        }

        // Create new executors to use - note that we do not need to create multiple executors
        // for work stealing, using the `spawn` global function that smol provides will work
        // just fine, as long as anything spawned by it is awaited from lua async functions
        let lua_exec = LocalExecutor::new();
        let main_exec = Arc::new(Executor::new());

        // Store the main executor in lua for spawner trait
        self.lua.set_app_data(Arc::downgrade(&main_exec));

        // Create a timer for a resumption cycle / throttling mechanism, waiting on this
        // will allow us to batch more work together when the runtime is under high load,
        // and adds an acceptable amount of latency for new async tasks (we run at 250hz)
        let mut cycle = Timer::interval(Duration::from_millis(4));

        // Tick local lua executor while also driving main
        // executor forward, until all lua threads finish
        let fut = async {
            loop {
                // Wait for a new thread to arrive __or__ next futures step, prioritizing
                // new threads, so we don't accidentally exit when there is more work to do
                let fut_spawn = self.queue_spawn.recv();
                let fut_defer = self.queue_defer.recv();
                let fut_tick = async {
                    lua_exec.tick().await;
                    // Do as much work as possible
                    loop {
                        if !lua_exec.try_tick() {
                            break;
                        }
                    }
                };

                fut_spawn.or(fut_defer).or(fut_tick).await;

                // If a new thread was spawned onto any queue,
                // we must drain them and schedule on the executor
                if self.queue_spawn.has_threads() || self.queue_defer.has_threads() {
                    let mut queued_threads = Vec::new();
                    queued_threads.extend(self.queue_spawn.drain(self.lua).await);
                    queued_threads.extend(self.queue_defer.drain(self.lua).await);
                    for (thread, args) in queued_threads {
                        // NOTE: Thread may have been cancelled from lua
                        // before we got here, so we need to check it again
                        if thread.status() == LuaThreadStatus::Resumable {
                            let mut stream = thread.clone().into_async::<_, LuaValue>(args);
                            lua_exec
                                .spawn(async move {
                                    // Only run stream until first coroutine.yield or completion. We will
                                    // drop it right away to clear stack space since detached tasks dont drop
                                    // until the executor drops https://github.com/smol-rs/smol/issues/294
                                    let res = stream.next().await.unwrap();
                                    if let Err(e) = &res {
                                        self.error_callback.call(e);
                                    }
                                    // TODO: Figure out how to give this result to caller of spawn_thread/defer_thread
                                })
                                .detach();
                        }
                    }
                }

                // Empty executor = no remaining threads
                if lua_exec.is_empty() {
                    break;
                }

                // Wait for next resumption cycle
                cycle.next().await;
            }
        };

        main_exec.run(fut).await;

        // Make sure we don't leave any references behind
        self.lua.remove_app_data::<Weak<Executor>>();
    }

    /**
        Runs the runtime until all Lua threads have completed, blocking the thread.

        See [`ThreadRuntime::run_async`] for more info.
    */
    pub fn run_blocking(&self) {
        block_on(self.run_async())
    }
}
