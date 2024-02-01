#![allow(clippy::module_name_repetitions)]

use std::{
    cell::Cell,
    process::ExitCode,
    rc::{Rc, Weak as WeakRc},
    sync::{Arc, Weak as WeakArc},
};

use futures_lite::prelude::*;
use mlua::prelude::*;

use async_executor::{Executor, LocalExecutor};
use tracing::Instrument;

use crate::{
    error_callback::ThreadErrorCallback,
    exit::Exit,
    queue::{DeferredThreadQueue, FuturesQueue, SpawnedThreadQueue},
    result_map::ThreadResultMap,
    status::Status,
    thread_id::ThreadId,
    traits::IntoLuaThread,
    util::{run_until_yield, ThreadResult},
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

const ERR_SET_CALLBACK_WHEN_RUNNING: &str = "\
Cannot set error callback when runtime is running!\
";

/**
    A runtime for running Lua threads and async tasks.
*/
#[derive(Clone)]
pub struct Runtime<'lua> {
    lua: &'lua Lua,
    queue_spawn: SpawnedThreadQueue,
    queue_defer: DeferredThreadQueue,
    error_callback: ThreadErrorCallback,
    result_map: ThreadResultMap,
    status: Rc<Cell<Status>>,
    exit: Exit,
}

impl<'lua> Runtime<'lua> {
    /**
        Creates a new runtime for the given Lua state.

        This runtime will have a default error callback that prints errors to stderr.

        # Panics

        Panics if the given Lua state already has a runtime attached to it.
    */
    #[must_use]
    pub fn new(lua: &'lua Lua) -> Runtime<'lua> {
        let queue_spawn = SpawnedThreadQueue::new();
        let queue_defer = DeferredThreadQueue::new();
        let error_callback = ThreadErrorCallback::default();
        let result_map = ThreadResultMap::new();
        let exit = Exit::new();

        assert!(
            lua.app_data_ref::<SpawnedThreadQueue>().is_none(),
            "{ERR_METADATA_ALREADY_ATTACHED}"
        );
        assert!(
            lua.app_data_ref::<DeferredThreadQueue>().is_none(),
            "{ERR_METADATA_ALREADY_ATTACHED}"
        );
        assert!(
            lua.app_data_ref::<ThreadErrorCallback>().is_none(),
            "{ERR_METADATA_ALREADY_ATTACHED}"
        );
        assert!(
            lua.app_data_ref::<ThreadResultMap>().is_none(),
            "{ERR_METADATA_ALREADY_ATTACHED}"
        );
        assert!(
            lua.app_data_ref::<Exit>().is_none(),
            "{ERR_METADATA_ALREADY_ATTACHED}"
        );

        lua.set_app_data(queue_spawn.clone());
        lua.set_app_data(queue_defer.clone());
        lua.set_app_data(error_callback.clone());
        lua.set_app_data(result_map.clone());
        lua.set_app_data(exit.clone());

        let status = Rc::new(Cell::new(Status::NotStarted));

        Runtime {
            lua,
            queue_spawn,
            queue_defer,
            error_callback,
            result_map,
            status,
            exit,
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

        # Panics

        Panics if the runtime is currently running.
    */
    pub fn set_error_callback(&self, callback: impl Fn(LuaError) + Send + 'static) {
        assert!(
            !self.status().is_running(),
            "{ERR_SET_CALLBACK_WHEN_RUNNING}"
        );
        self.error_callback.replace(callback);
    }

    /**
        Clears the error callback for this runtime.

        This will remove any current error callback, including default(s).

        # Panics

        Panics if the runtime is currently running.
    */
    pub fn remove_error_callback(&self) {
        assert!(
            !self.status().is_running(),
            "{ERR_SET_CALLBACK_WHEN_RUNNING}"
        );
        self.error_callback.clear();
    }

    /**
        Gets the exit code for this runtime, if one has been set.
    */
    #[must_use]
    pub fn get_exit_code(&self) -> Option<ExitCode> {
        self.exit.get()
    }

    /**
        Sets the exit code for this runtime.

        This will cause [`Runtime::run`] to exit immediately.
    */
    pub fn set_exit_code(&self, code: ExitCode) {
        self.exit.set(code);
    }

    /**
        Spawns a chunk / function / thread onto the runtime queue.

        Threads are guaranteed to be resumed in the order that they were pushed to the queue.

        # Returns

        Returns a [`ThreadId`] that can be used to retrieve the result of the thread.

        Note that the result may not be available until [`Runtime::run`] completes.

        # Errors

        Errors when out of memory.
    */
    pub fn push_thread_front(
        &self,
        thread: impl IntoLuaThread<'lua>,
        args: impl IntoLuaMulti<'lua>,
    ) -> LuaResult<ThreadId> {
        tracing::debug!(deferred = false, "new runtime thread");
        let id = self.queue_spawn.push_item(self.lua, thread, args)?;
        self.result_map.track(id);
        Ok(id)
    }

    /**
        Defers a chunk / function / thread onto the runtime queue.

        Deferred threads are guaranteed to run after all spawned threads either yield or complete.

        Threads are guaranteed to be resumed in the order that they were pushed to the queue.

        # Returns

        Returns a [`ThreadId`] that can be used to retrieve the result of the thread.

        Note that the result may not be available until [`Runtime::run`] completes.

        # Errors

        Errors when out of memory.
    */
    pub fn push_thread_back(
        &self,
        thread: impl IntoLuaThread<'lua>,
        args: impl IntoLuaMulti<'lua>,
    ) -> LuaResult<ThreadId> {
        tracing::debug!(deferred = true, "new runtime thread");
        let id = self.queue_defer.push_item(self.lua, thread, args)?;
        self.result_map.track(id);
        Ok(id)
    }

    /**
        Gets the tracked result for the [`LuaThread`] with the given [`ThreadId`].

        Depending on the current [`Runtime::status`], this method will return:

        - [`Status::NotStarted`]: returns `None`.
        - [`Status::Running`]: may return `Some(Ok(v))` or `Some(Err(e))`, but it is not guaranteed.
        - [`Status::Completed`]: returns `Some(Ok(v))` or `Some(Err(e))`.

        Note that this method also takes the value out of the runtime and
        stops tracking the given thread, so it may only be called once.

        Any subsequent calls after this method returns `Some` will return `None`.
    */
    #[must_use]
    pub fn get_thread_result(&self, id: ThreadId) -> Option<LuaResult<LuaMultiValue<'lua>>> {
        self.result_map.remove(id).map(|r| r.value(self.lua))
    }

    /**
        Waits for the [`LuaThread`] with the given [`ThreadId`] to complete.

        This will return instantly if the thread has already completed.
    */
    pub async fn wait_for_thread(&self, id: ThreadId) {
        self.result_map.listen(id).await;
    }

    /**
        Runs the runtime until all Lua threads have completed.

        Note that the given Lua state must be the same one that was
        used to create this runtime, otherwise this method will panic.

        # Panics

        Panics if the given Lua state already has a runtime attached to it.
    */
    #[allow(clippy::too_many_lines)]
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
        let local_exec = LocalExecutor::new();
        let main_exec = Arc::new(Executor::new());
        let fut_queue = Rc::new(FuturesQueue::new());

        /*
            Store the main executor and queue in Lua, so that they may be used with LuaRuntimeExt.

            Also ensure we do not already have an executor or queues - these are definite user errors
            and may happen if the user tries to run multiple runtimes on the same Lua state at once.
        */
        assert!(
            self.lua.app_data_ref::<WeakArc<Executor>>().is_none(),
            "{ERR_METADATA_ALREADY_ATTACHED}"
        );
        assert!(
            self.lua.app_data_ref::<WeakRc<FuturesQueue>>().is_none(),
            "{ERR_METADATA_ALREADY_ATTACHED}"
        );

        self.lua.set_app_data(Arc::downgrade(&main_exec));
        self.lua.set_app_data(Rc::downgrade(&fut_queue.clone()));

        /*
            Manually tick the Lua executor, while running under the main executor.
            Each tick we wait for the next action to perform, in prioritized order:

            1. The exit event is triggered by setting an exit code
            2. A Lua thread is available to run on the spawned queue
            3. A Lua thread is available to run on the deferred queue
            4. A new thread-local future is available to run on the local executor
            5. Task(s) scheduled on the Lua executor have made progress and should be polled again

            This ordering is vital to ensure that we don't accidentally exit the main loop
            when there are new Lua threads to enqueue and potentially more work to be done.
        */
        let fut = async {
            let result_map = self.result_map.clone();
            let process_thread = |thread: LuaThread<'lua>, args| {
                // NOTE: Thread may have been cancelled from Lua
                // before we got here, so we need to check it again
                if thread.status() == LuaThreadStatus::Resumable {
                    // Check if we should be tracking this thread
                    let id = ThreadId::from(&thread);
                    let id_tracked = result_map.is_tracked(id);
                    let result_map_inner = if id_tracked {
                        Some(result_map.clone())
                    } else {
                        None
                    };
                    // Create our future which will run the thread and store its final result
                    let fut = async move {
                        if id_tracked {
                            // Run until yield and check if we got a final result
                            if let Some(res) = run_until_yield(thread.clone(), args).await {
                                if let Err(e) = res.as_ref() {
                                    self.error_callback.call(e);
                                }
                                if thread.status() != LuaThreadStatus::Resumable {
                                    let thread_res = ThreadResult::new(res, self.lua);
                                    result_map_inner.unwrap().insert(id, thread_res);
                                }
                            }
                        } else {
                            // Just run until yield
                            if let Some(res) = run_until_yield(thread, args).await {
                                if let Err(e) = res.as_ref() {
                                    self.error_callback.call(e);
                                }
                            }
                        }
                    };
                    // Spawn it on the executor
                    local_exec.spawn(fut).detach();
                }
            };

            loop {
                let fut_exit = self.exit.listen(); // 1
                let fut_spawn = self.queue_spawn.wait_for_item(); // 2
                let fut_defer = self.queue_defer.wait_for_item(); // 3
                let fut_futs = fut_queue.wait_for_item(); // 4

                // 5
                let mut num_processed = 0;
                let span_tick = tracing::debug_span!("tick_executor");
                let fut_tick = async {
                    local_exec.tick().await;
                    // NOTE: Try to do as much work as possible instead of just a single tick()
                    num_processed += 1;
                    while local_exec.try_tick() {
                        num_processed += 1;
                    }
                };

                // 1 + 2 + 3 + 4 + 5
                fut_exit
                    .or(fut_spawn)
                    .or(fut_defer)
                    .or(fut_futs)
                    .or(fut_tick.instrument(span_tick.or_current()))
                    .await;

                // Check if we should exit
                if self.exit.get().is_some() {
                    tracing::trace!("exited with code");
                    break;
                }

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

                // Process spawned futures
                let mut num_futs = 0;
                for fut in fut_queue.drain_items() {
                    local_exec.spawn(fut).detach();
                    num_futs += 1;
                }
                if num_futs > 0 {
                    tracing::trace!(num_futs, "futures_spawned");
                }

                // Empty executor = we didn't spawn any new Lua tasks
                // above, and there are no remaining tasks to run later
                if local_exec.is_empty() {
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
            .remove_app_data::<WeakArc<Executor>>()
            .expect(ERR_METADATA_REMOVED);
        self.lua
            .remove_app_data::<WeakRc<FuturesQueue>>()
            .expect(ERR_METADATA_REMOVED);
    }
}

impl Drop for Runtime<'_> {
    fn drop(&mut self) {
        self.lua
            .remove_app_data::<SpawnedThreadQueue>()
            .expect(ERR_METADATA_REMOVED);
        self.lua
            .remove_app_data::<DeferredThreadQueue>()
            .expect(ERR_METADATA_REMOVED);
        self.lua
            .remove_app_data::<ThreadErrorCallback>()
            .expect(ERR_METADATA_REMOVED);
        self.lua
            .remove_app_data::<ThreadResultMap>()
            .expect(ERR_METADATA_REMOVED);
        self.lua
            .remove_app_data::<Exit>()
            .expect(ERR_METADATA_REMOVED);
    }
}
