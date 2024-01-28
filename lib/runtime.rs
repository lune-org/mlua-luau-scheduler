#![allow(clippy::module_name_repetitions)]

use std::{
    cell::Cell,
    rc::Rc,
    sync::{Arc, Weak},
};

use futures_lite::prelude::*;
use mlua::prelude::*;

use async_executor::{Executor, LocalExecutor};
use tracing::Instrument;

use crate::{
    error_callback::ThreadErrorCallback,
    handle::Handle,
    queue::{DeferredThreadQueue, SpawnedThreadQueue},
    status::Status,
    traits::IntoLuaThread,
    util::{run_until_yield, LuaThreadOrFunction},
};

const ERR_METADATA_ALREADY_ATTACHED: &str = "\
Lua state already has runtime metadata attached!\
\nThis may be caused by running multiple runtimes on the same Lua state, or a call to Runtime::run being cancelled.\
\nOnly one runtime can be used per Lua state at once, and runtimes must always run until completion.\
";

const ERR_METADATA_REMOVED: &str = "\
Lua state runtime metadata was unexpectedly removed!\
\nThis should never happen, and is likely a bug in the runtime.\
";

/**
    A runtime for running Lua threads and async tasks.
*/
pub struct Runtime<'lua> {
    lua: &'lua Lua,
    queue_spawn: SpawnedThreadQueue,
    queue_defer: DeferredThreadQueue,
    error_callback: ThreadErrorCallback,
    status: Rc<Cell<Status>>,
}

impl<'lua> Runtime<'lua> {
    /**
        Creates a new runtime for the given Lua state.

        This runtime will have a default error callback that prints errors to stderr.
    */
    #[must_use]
    pub fn new(lua: &'lua Lua) -> Runtime<'lua> {
        let queue_spawn = SpawnedThreadQueue::new();
        let queue_defer = DeferredThreadQueue::new();
        let error_callback = ThreadErrorCallback::default();
        let status = Rc::new(Cell::new(Status::NotStarted));
        Runtime {
            lua,
            queue_spawn,
            queue_defer,
            error_callback,
            status,
        }
    }

    /**
        Returns the current status of this runtime.
    */
    #[must_use]
    pub fn status(&self) -> Status {
        self.status.get()
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
        Creates a collection of lua functions that may be called to interact with the runtime.

        # Errors

        Errors when out of memory.
    */
    pub fn create_functions(&self) -> LuaResult<Functions> {
        Functions::new(self)
    }

    /**
        Spawns a chunk / function / thread onto the runtime queue.

        Threads are guaranteed to be resumed in the order that they were pushed to the queue.

        # Returns

        Returns a [`Handle`] that can be used to retrieve the result of the thread.

        Note that the result may not be available until [`Runtime::run`] completes.

        # Errors

        Errors when out of memory.
    */
    pub fn push_thread_front(
        &self,
        thread: impl IntoLuaThread<'lua>,
        args: impl IntoLuaMulti<'lua>,
    ) -> LuaResult<Handle> {
        tracing::debug!(deferred = false, "new runtime thread");
        self.queue_spawn
            .push_item_with_handle(self.lua, thread, args)
    }

    /**
        Defers a chunk / function / thread onto the runtime queue.

        Deferred threads are guaranteed to run after all spawned threads either yield or complete.

        Threads are guaranteed to be resumed in the order that they were pushed to the queue.

        # Returns

        Returns a [`Handle`] that can be used to retrieve the result of the thread.

        Note that the result may not be available until [`Runtime::run`] completes.

        # Errors

        Errors when out of memory.
    */
    pub fn push_thread_back(
        &self,
        thread: impl IntoLuaThread<'lua>,
        args: impl IntoLuaMulti<'lua>,
    ) -> LuaResult<Handle> {
        tracing::debug!(deferred = true, "new runtime thread");
        self.queue_defer
            .push_item_with_handle(self.lua, thread, args)
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
            Store the main executor and queues in Lua, so that they may be used with LuaRuntimeExt.

            Also ensure we do not already have an executor or queues - these are definite user errors
            and may happen if the user tries to run multiple runtimes on the same Lua state at once.
        */
        assert!(
            self.lua.app_data_ref::<Weak<Executor>>().is_none(),
            "{ERR_METADATA_ALREADY_ATTACHED}"
        );
        assert!(
            self.lua.app_data_ref::<SpawnedThreadQueue>().is_none(),
            "{ERR_METADATA_ALREADY_ATTACHED}"
        );
        assert!(
            self.lua.app_data_ref::<DeferredThreadQueue>().is_none(),
            "{ERR_METADATA_ALREADY_ATTACHED}"
        );
        self.lua.set_app_data(Arc::downgrade(&main_exec));
        self.lua.set_app_data(self.queue_spawn.clone());
        self.lua.set_app_data(self.queue_defer.clone());

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
                    lua_exec
                        .spawn(async move {
                            if let Err(e) = run_until_yield(thread, args).await {
                                self.error_callback.call(&e);
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
        self.status.set(Status::Running);
        tracing::debug!("starting runtime");

        let span = tracing::debug_span!("run_executor");
        main_exec.run(fut).instrument(span.or_current()).await;

        tracing::debug!("runtime completed");
        self.status.set(Status::Completed);

        // Clean up
        self.lua
            .remove_app_data::<Weak<Executor>>()
            .expect(ERR_METADATA_REMOVED);
        self.lua
            .remove_app_data::<SpawnedThreadQueue>()
            .expect(ERR_METADATA_REMOVED);
        self.lua
            .remove_app_data::<DeferredThreadQueue>()
            .expect(ERR_METADATA_REMOVED);
    }
}

/**
    A collection of lua functions that may be called to interact with a [`Runtime`].
*/
pub struct Functions<'lua> {
    /**
        Spawns a function / thread onto the runtime queue.
        Resumes once instantly, and runs until first yield.
        Adds to the queue if not completed.
    */
    pub spawn: LuaFunction<'lua>,
    /**
        Defers a function / thread onto the runtime queue.
        Does not resume instantly, only adds to the queue.
    */
    pub defer: LuaFunction<'lua>,
    /**
        Cancels a function / thread, removing it from the queue.
    */
    pub cancel: LuaFunction<'lua>,
}

impl<'lua> Functions<'lua> {
    fn new(rt: &Runtime<'lua>) -> LuaResult<Self> {
        let error_callback = rt.error_callback.clone();
        let spawn_queue = rt.queue_spawn.clone();
        let spawn = rt.lua.create_function(
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
        )?;

        let defer_queue = rt.queue_defer.clone();
        let defer = rt.lua.create_function(
            move |lua, (tof, args): (LuaThreadOrFunction, LuaMultiValue)| {
                let thread = tof.into_thread(lua)?;
                if thread.status() == LuaThreadStatus::Resumable {
                    defer_queue.push_item(lua, &thread, args)?;
                }
                Ok(thread)
            },
        )?;

        let close = rt
            .lua
            .globals()
            .get::<_, LuaTable>("coroutine")?
            .get::<_, LuaFunction>("close")?;
        let close_key = rt.lua.create_registry_value(close)?;
        let cancel = rt.lua.create_function(move |lua, thread: LuaThread| {
            let close: LuaFunction = lua.registry_value(&close_key)?;
            match close.call(thread) {
                Err(LuaError::CoroutineInactive) | Ok(()) => Ok(()),
                Err(e) => Err(e),
            }
        })?;

        Ok(Self {
            spawn,
            defer,
            cancel,
        })
    }
}
