#![allow(unused_imports)]
#![allow(clippy::module_name_repetitions)]

use mlua::prelude::*;

use crate::{
    error_callback::ThreadErrorCallback,
    queue::{DeferredThreadQueue, SpawnedThreadQueue},
    runtime::Runtime,
    util::LuaThreadOrFunction,
};

const ERR_METADATA_NOT_ATTACHED: &str = "\
Lua state does not have runtime metadata attached!\
\nThis is most likely caused by creating functions outside of a runtime.\
\nRuntime functions must always be created from within an active runtime.\
";

/**
    A collection of lua functions that may be called to interact with a [`Runtime`].
*/
pub struct Functions<'lua> {
    /**
        Resumes a function / thread once instantly, and runs until first yield.

        Spawns onto the runtime queue if not completed.
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
    /**
        Creates a new collection of Lua functions that may be called to interact with a [`Runtime`].

        # Errors

        Errors when out of memory, or if default Lua globals are missing.

        # Panics

        Panics when the given [`Lua`] instance does not have an attached [`Runtime`].
    */
    pub fn new(lua: &'lua Lua) -> LuaResult<Self> {
        let spawn_queue = lua
            .app_data_ref::<SpawnedThreadQueue>()
            .expect(ERR_METADATA_NOT_ATTACHED)
            .clone();
        let defer_queue = lua
            .app_data_ref::<DeferredThreadQueue>()
            .expect(ERR_METADATA_NOT_ATTACHED)
            .clone();
        let error_callback = lua
            .app_data_ref::<ThreadErrorCallback>()
            .expect(ERR_METADATA_NOT_ATTACHED)
            .clone();

        let spawn = lua.create_function(
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

        let defer = lua.create_function(
            move |lua, (tof, args): (LuaThreadOrFunction, LuaMultiValue)| {
                let thread = tof.into_thread(lua)?;
                if thread.status() == LuaThreadStatus::Resumable {
                    defer_queue.push_item(lua, &thread, args)?;
                }
                Ok(thread)
            },
        )?;

        let close = lua
            .globals()
            .get::<_, LuaTable>("coroutine")?
            .get::<_, LuaFunction>("close")?;
        let close_key = lua.create_registry_value(close)?;
        let cancel = lua.create_function(move |lua, thread: LuaThread| {
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
