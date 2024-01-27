use std::sync::{Arc, Weak};

use futures_lite::prelude::*;
use mlua::prelude::*;

use async_executor::{Executor, LocalExecutor};
use tracing::Instrument;

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
    #[must_use]
    pub fn new(lua: &'lua Lua) -> Runtime<'lua> {
        let queue_spawn = ThreadQueue::new();
        let queue_defer = ThreadQueue::new();
        let error_callback = ThreadErrorCallback::default();

        Runtime {
            lua,
            queue_spawn,
            queue_defer,
            error_callback,
        }
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

        # Errors

        Errors when out of memory.
    */
    pub fn spawn_thread(
        &self,
        thread: impl IntoLuaThread<'lua>,
        args: impl IntoLuaMulti<'lua>,
    ) -> LuaResult<()> {
        tracing::debug!(deferred = false, "new runtime thread");
        self.queue_spawn.push_item(self.lua, thread, args)
    }

    /**
        Defers a chunk / function / thread onto the runtime queue.

        Deferred threads are guaranteed to run after all spawned threads either yield or complete.

        Threads are guaranteed to be resumed in the order that they were pushed to the queue.

        # Errors

        Errors when out of memory.
    */
    pub fn defer_thread(
        &self,
        thread: impl IntoLuaThread<'lua>,
        args: impl IntoLuaMulti<'lua>,
    ) -> LuaResult<()> {
        tracing::debug!(deferred = true, "new runtime thread");
        self.queue_defer.push_item(self.lua, thread, args)
    }

    /**
        Creates a Lua function that can be used to spawn threads / functions onto the runtime queue.

        The function takes a thread or function as the first argument, and any variadic arguments as the rest.

        # Errors

        Errors when out of memory.
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
                                spawn_queue.push_item(lua, &thread, args)?;
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
        Creates a Lua function that can be used to defer threads / functions onto the runtime queue.

        The function takes a thread or function as the first argument, and any variadic arguments as the rest.

        Deferred threads are guaranteed to run after all spawned threads either yield or complete.

        # Errors

        Errors when out of memory.
    */
    pub fn create_defer_function(&self) -> LuaResult<LuaFunction<'lua>> {
        let defer_queue = self.queue_defer.clone();
        self.lua.create_function(
            move |lua, (tof, args): (LuaThreadOrFunction, LuaMultiValue)| {
                let thread = tof.into_thread(lua)?;
                if thread.status() == LuaThreadStatus::Resumable {
                    defer_queue.push_item(lua, &thread, args)?;
                }
                Ok(thread)
            },
        )
    }

    /**
        Runs the runtime until all Lua threads have completed.

        Note that the given Lua state must be the same one that was
        used to create this runtime, otherwise this method will panic.

        # Panics

        Panics if the given Lua state already has a runtime attached to it.
    */
    pub async fn run(&self) {
        /*
            Create new executors to use - note that we do not need create multiple executors
            for work stealing, the user may do that themselves if they want to and it will work
            just fine, as long as anything async is .await-ed from within a Lua async function.

            The main purpose of the two executors here is just to have one with
            the Send bound, and another (local) one without it, for Lua scheduling.

            We also use the main executor to drive the main loop below forward,
            saving a tiny bit of processing from going on the Lua executor itself.
        */
        let lua_exec = LocalExecutor::new();
        let main_exec = Arc::new(Executor::new());

        /*
            Store the main executor in Lua, so that it may be used with LuaSpawnExt.

            Also ensure we do not already have an executor - this is a definite user error
            and may happen if the user tries to run multiple runtimes on the same Lua state.
        */
        assert!(
            self.lua.app_data_ref::<Weak<Executor>>().is_none(),
            "\
            Lua state already has an executor attached!\
            \nThis may be caused by running multiple runtimes on the same Lua state, or a call to Runtime::run being cancelled.\
            \nOnly one runtime can be used per Lua state at once, and runtimes must always run until completion.\
            "
        );
        self.lua.set_app_data(Arc::downgrade(&main_exec));

        /*
            Manually tick the Lua executor, while running under the main executor.
            Each tick we wait for the next action to perform, in prioritized order:

            1. A Lua thread is available to run on the spawned queue
            2. A Lua thread is available to run on the deferred queue
            3. Task(s) scheduled on the Lua executor have made progress and should be polled again

            This ordering is vital to ensure that we don't accidentally exit the main loop
            when there are new Lua threads to enqueue and potentially more work to be done.
        */
        let fut = async {
            let process_thread = |thread: LuaThread<'lua>, args| {
                // NOTE: Thread may have been cancelled from Lua
                // before we got here, so we need to check it again
                if thread.status() == LuaThreadStatus::Resumable {
                    let mut stream = thread.clone().into_async::<_, LuaValue>(args);
                    lua_exec
                        .spawn(async move {
                            // Only run stream until first coroutine.yield or completion. We will
                            // drop it right away to clear stack space since detached tasks dont drop
                            // until the executor drops (https://github.com/smol-rs/smol/issues/294)
                            let res = stream.next().await.unwrap();
                            if let Err(e) = &res {
                                self.error_callback.call(e);
                            }
                        })
                        .detach();
                }
            };

            loop {
                let fut_spawn = self.queue_spawn.wait_for_item(); // 1
                let fut_defer = self.queue_defer.wait_for_item(); // 2

                // 3
                let mut num_processed = 0;
                let span_tick = tracing::debug_span!("tick_executor");
                let fut_tick = async {
                    lua_exec.tick().await;
                    // NOTE: Try to do as much work as possible instead of just a single tick()
                    num_processed += 1;
                    while lua_exec.try_tick() {
                        num_processed += 1;
                    }
                };

                // 1 + 2 + 3
                fut_spawn
                    .or(fut_defer)
                    .or(fut_tick.instrument(span_tick.or_current()))
                    .await;

                // Emit traces
                if num_processed > 0 {
                    tracing::trace!(num_processed, "tasks_processed");
                }

                // Process spawned threads first, then deferred threads
                let mut num_spawned = 0;
                let mut num_deferred = 0;
                for (thread, args) in self.queue_spawn.drain_items(self.lua) {
                    process_thread(thread, args);
                    num_spawned += 1;
                }
                for (thread, args) in self.queue_defer.drain_items(self.lua) {
                    process_thread(thread, args);
                    num_deferred += 1;
                }
                if num_spawned > 0 || num_deferred > 0 {
                    tracing::trace!(num_spawned, num_deferred, "tasks_spawned");
                }

                // Empty executor = we didn't spawn any new Lua tasks
                // above, and there are no remaining tasks to run later
                if lua_exec.is_empty() {
                    break;
                }
            }
        };

        // Run the executor inside a span until all lua threads complete
        let span = tracing::debug_span!("run_executor");
        main_exec.run(fut).instrument(span.or_current()).await;

        // Clean up
        self.lua.remove_app_data::<Weak<Executor>>();
    }
}
