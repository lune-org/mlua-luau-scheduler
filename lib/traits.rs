#![allow(clippy::missing_errors_doc)]

use std::{future::Future, sync::Weak};

use mlua::prelude::*;

use async_executor::{Executor, Task};

use crate::{
    handle::Handle,
    queue::{DeferredThreadQueue, SpawnedThreadQueue},
};

/**
    Trait for any struct that can be turned into an [`LuaThread`]
    and passed to the runtime, implemented for the following types:

    - Lua threads ([`LuaThread`])
    - Lua functions ([`LuaFunction`])
    - Lua chunks ([`LuaChunk`])
*/
pub trait IntoLuaThread<'lua> {
    /**
        Converts the value into a Lua thread.

        # Errors

        Errors when out of memory.
    */
    fn into_lua_thread(self, lua: &'lua Lua) -> LuaResult<LuaThread<'lua>>;
}

impl<'lua> IntoLuaThread<'lua> for LuaThread<'lua> {
    fn into_lua_thread(self, _: &'lua Lua) -> LuaResult<LuaThread<'lua>> {
        Ok(self)
    }
}

impl<'lua> IntoLuaThread<'lua> for LuaFunction<'lua> {
    fn into_lua_thread(self, lua: &'lua Lua) -> LuaResult<LuaThread<'lua>> {
        lua.create_thread(self)
    }
}

impl<'lua> IntoLuaThread<'lua> for LuaChunk<'lua, '_> {
    fn into_lua_thread(self, lua: &'lua Lua) -> LuaResult<LuaThread<'lua>> {
        lua.create_thread(self.into_function()?)
    }
}

impl<'lua, T> IntoLuaThread<'lua> for &T
where
    T: IntoLuaThread<'lua> + Clone,
{
    fn into_lua_thread(self, lua: &'lua Lua) -> LuaResult<LuaThread<'lua>> {
        self.clone().into_lua_thread(lua)
    }
}

/**
    Trait for scheduling Lua threads and spawning `Send` futures on the current executor.

    For spawning `!Send` futures on the same local executor as a [`Lua`]
    VM instance, [`Lua::create_async_function`] should be used instead.
*/
pub trait LuaRuntimeExt<'lua> {
    /**
        Pushes (spawns) a lua thread to the **front** of the current runtime.

        See [`Runtime::push_thread_front`] for more information.

        # Panics

        Panics if called outside of a running [`Runtime`].
    */
    fn push_thread_front(
        &'lua self,
        thread: impl IntoLuaThread<'lua>,
        args: impl IntoLuaMulti<'lua>,
    ) -> LuaResult<Handle>;

    /**
        Pushes (defers) a lua thread to the **back** of the current runtime.

        See [`Runtime::push_thread_back`] for more information.

        # Panics

        Panics if called outside of a running [`Runtime`].
    */
    fn push_thread_back(
        &'lua self,
        thread: impl IntoLuaThread<'lua>,
        args: impl IntoLuaMulti<'lua>,
    ) -> LuaResult<Handle>;

    /**
        Spawns the given future on the current executor and returns its [`Task`].

        # Panics

        Panics if called outside of a running [`Runtime`].

        # Example usage

        ```rust
        use async_io::block_on;

        use mlua::prelude::*;
        use mlua_luau_runtime::*;

        fn main() -> LuaResult<()> {
            let lua = Lua::new();

            lua.globals().set(
                "spawnBackgroundTask",
                lua.create_async_function(|lua, ()| async move {
                    lua.spawn_future(async move {
                        println!("Hello from background task!");
                    }).await;
                    Ok(())
                })?
            )?;

            let rt = Runtime::new(&lua);
            rt.push_thread_front(lua.load("spawnBackgroundTask()"), ());
            block_on(rt.run());

            Ok(())
        }
        ```

        [`Runtime`]: crate::Runtime
    */
    fn spawn<T: Send + 'static>(&self, fut: impl Future<Output = T> + Send + 'static) -> Task<T>;
}

impl<'lua> LuaRuntimeExt<'lua> for Lua {
    fn push_thread_front(
        &'lua self,
        thread: impl IntoLuaThread<'lua>,
        args: impl IntoLuaMulti<'lua>,
    ) -> LuaResult<Handle> {
        let queue = self
            .app_data_ref::<SpawnedThreadQueue>()
            .expect("lua threads can only be pushed within a runtime");
        queue.push_item_with_handle(self, thread, args)
    }

    fn push_thread_back(
        &'lua self,
        thread: impl IntoLuaThread<'lua>,
        args: impl IntoLuaMulti<'lua>,
    ) -> LuaResult<Handle> {
        let queue = self
            .app_data_ref::<DeferredThreadQueue>()
            .expect("lua threads can only be pushed within a runtime");
        queue.push_item_with_handle(self, thread, args)
    }

    fn spawn<T: Send + 'static>(&self, fut: impl Future<Output = T> + Send + 'static) -> Task<T> {
        let exec = self
            .app_data_ref::<Weak<Executor>>()
            .expect("futures can only be spawned within a runtime")
            .upgrade()
            .expect("executor was dropped");
        tracing::trace!("spawning future on executor");
        exec.spawn(fut)
    }
}
