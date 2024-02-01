#![allow(unused_imports)]
#![allow(clippy::too_many_lines)]

use std::process::ExitCode;

use mlua::prelude::*;

use crate::{
    error_callback::ThreadErrorCallback,
    queue::{DeferredThreadQueue, SpawnedThreadQueue},
    result_map::ThreadResultMap,
    runtime::Runtime,
    thread_id::ThreadId,
    traits::LuaRuntimeExt,
    util::{is_poll_pending, LuaThreadOrFunction, ThreadResult},
};

const ERR_METADATA_NOT_ATTACHED: &str = "\
Lua state does not have runtime metadata attached!\
\nThis is most likely caused by creating functions outside of a runtime.\
\nRuntime functions must always be created from within an active runtime.\
";

const EXIT_IMPL_LUA: &str = r"
exit(...)
yield()
";

const WRAP_IMPL_LUA: &str = r"
local t = create(...)
return function(...)
    local r = { resume(t, ...) }
    if r[1] then
        return select(2, unpack(r))
    else
        error(r[2], 2)
    end
end
";

/**
    A collection of lua functions that may be called to interact with a [`Runtime`].
*/
pub struct Functions<'lua> {
    /**
        Implementation of `coroutine.resume` that handles async polling properly.

        Defers onto the runtime queue if the thread calls an async function.
    */
    pub resume: LuaFunction<'lua>,
    /**
        Implementation of `coroutine.wrap` that handles async polling properly.

        Defers onto the runtime queue if the thread calls an async function.
    */
    pub wrap: LuaFunction<'lua>,
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
    /**
        Exits the runtime, stopping all other threads and closing the runtime.

        Yields the calling thread to ensure that it does not continue.
    */
    pub exit: LuaFunction<'lua>,
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
        let result_map = lua
            .app_data_ref::<ThreadResultMap>()
            .expect(ERR_METADATA_NOT_ATTACHED)
            .clone();

        let resume_queue = defer_queue.clone();
        let resume_map = result_map.clone();
        let resume =
            lua.create_function(move |lua, (thread, args): (LuaThread, LuaMultiValue)| {
                match thread.resume::<_, LuaMultiValue>(args.clone()) {
                    Ok(v) => {
                        if v.get(0).map(is_poll_pending).unwrap_or_default() {
                            // Pending, defer to scheduler and return nil
                            resume_queue.push_item(lua, &thread, args)?;
                            (true, LuaValue::Nil).into_lua_multi(lua)
                        } else {
                            // Not pending, store the value if thread is done
                            if thread.status() != LuaThreadStatus::Resumable {
                                let id = ThreadId::from(&thread);
                                if resume_map.is_tracked(id) {
                                    let res = ThreadResult::new(Ok(v.clone()), lua);
                                    resume_map.insert(id, res);
                                }
                            }
                            (true, v).into_lua_multi(lua)
                        }
                    }
                    Err(e) => {
                        // Not pending, store the error
                        let id = ThreadId::from(&thread);
                        if resume_map.is_tracked(id) {
                            let res = ThreadResult::new(Err(e.clone()), lua);
                            resume_map.insert(id, res);
                        }
                        (false, e.to_string()).into_lua_multi(lua)
                    }
                }
            })?;

        let wrap_env = lua.create_table_from(vec![
            ("resume", resume.clone()),
            ("error", lua.globals().get::<_, LuaFunction>("error")?),
            ("select", lua.globals().get::<_, LuaFunction>("select")?),
            ("unpack", lua.globals().get::<_, LuaFunction>("unpack")?),
            (
                "create",
                lua.globals()
                    .get::<_, LuaTable>("coroutine")?
                    .get::<_, LuaFunction>("create")?,
            ),
        ])?;
        let wrap = lua
            .load(WRAP_IMPL_LUA)
            .set_name("=__runtime_wrap")
            .set_environment(wrap_env)
            .into_function()?;

        let spawn_map = result_map.clone();
        let spawn = lua.create_function(
            move |lua, (tof, args): (LuaThreadOrFunction, LuaMultiValue)| {
                let thread = tof.into_thread(lua)?;
                if thread.status() == LuaThreadStatus::Resumable {
                    // NOTE: We need to resume the thread once instantly for correct behavior,
                    // and only if we get the pending value back we can spawn to async executor
                    match thread.resume::<_, LuaMultiValue>(args.clone()) {
                        Ok(v) => {
                            if v.get(0).map(is_poll_pending).unwrap_or_default() {
                                spawn_queue.push_item(lua, &thread, args)?;
                            } else {
                                // Not pending, store the value if thread is done
                                if thread.status() != LuaThreadStatus::Resumable {
                                    let id = ThreadId::from(&thread);
                                    if spawn_map.is_tracked(id) {
                                        let res = ThreadResult::new(Ok(v), lua);
                                        spawn_map.insert(id, res);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error_callback.call(&e);
                            // Not pending, store the error
                            let id = ThreadId::from(&thread);
                            if spawn_map.is_tracked(id) {
                                let res = ThreadResult::new(Err(e), lua);
                                spawn_map.insert(id, res);
                            }
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

        let exit_env = lua.create_table_from(vec![
            (
                "exit",
                lua.create_function(|lua, code: Option<u8>| {
                    let code = code.map(ExitCode::from).unwrap_or_default();
                    lua.set_exit_code(code);
                    Ok(())
                })?,
            ),
            (
                "yield",
                lua.globals()
                    .get::<_, LuaTable>("coroutine")?
                    .get::<_, LuaFunction>("yield")?,
            ),
        ])?;
        let exit = lua
            .load(EXIT_IMPL_LUA)
            .set_name("=__runtime_exit")
            .set_environment(exit_env)
            .into_function()?;

        Ok(Self {
            resume,
            wrap,
            spawn,
            defer,
            cancel,
            exit,
        })
    }
}

impl Functions<'_> {
    /**
        Injects [`Runtime`]-compatible functions into the given [`Lua`] instance.

        This will overwrite the following functions:

        - `coroutine.resume`
        - `coroutine.wrap`

        # Errors

        Errors when out of memory, or if default Lua globals are missing.
    */
    pub fn inject_compat(&self, lua: &Lua) -> LuaResult<()> {
        let co: LuaTable = lua.globals().get("coroutine")?;
        co.set("resume", self.resume.clone())?;
        co.set("wrap", self.wrap.clone())?;
        Ok(())
    }
}
