use std::{future::Future, sync::Weak};

use mlua::prelude::*;
use smol::{Executor, Task};

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

/**
    Trait for spawning `Send` futures on the current executor.

    For spawning non-`Send` futures on the same local executor as a [`Lua`]
    VM instance, [`Lua::create_async_function`] should be used instead.
*/
pub trait LuaExecutorExt<'lua> {
    /**
        Spawns the given future on the current executor and returns its [`Task`].

        ### Panics

        Panics if called outside of a [`Runtime`].

        ### Example usage

        ```rust
        use mlua::prelude::*;
        use smol_mlua::{Runtime, LuaExecutorExt};

        fn main() -> LuaResult<()> {
            let lua = Lua::new();

            lua.globals().set(
                "spawnBackgroundTask",
                lua.create_async_function(|lua, ()| async move {
                    lua.spawn(async move {
                        println!("Hello from background task!");
                    }).await;
                    Ok(())
                })?
            )?;

            let rt = Runtime::new(&lua)?;
            rt.push_thread(lua.load("spawnBackgroundTask()"), ());
            rt.run_blocking();

            Ok(())
        }
        ```

        [`Runtime`]: crate::Runtime
    */
    fn spawn<T: Send + 'static>(&self, fut: impl Future<Output = T> + Send + 'static) -> Task<T>;
}

impl<'lua> LuaExecutorExt<'lua> for Lua {
    fn spawn<T: Send + 'static>(&self, fut: impl Future<Output = T> + Send + 'static) -> Task<T> {
        let exec = self
            .app_data_ref::<Weak<Executor>>()
            .expect("futures can only be spawned within a runtime")
            .upgrade()
            .expect("executor was dropped");
        exec.spawn(fut)
    }
}
