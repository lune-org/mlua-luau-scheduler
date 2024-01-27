use std::{future::Future, sync::Weak};

use mlua::prelude::*;

use async_executor::{Executor, Task};

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
    Trait for spawning `Send` futures on the current executor.

    For spawning `!Send` futures on the same local executor as a [`Lua`]
    VM instance, [`Lua::create_async_function`] should be used instead.
*/
pub trait LuaSpawnExt<'lua> {
    /**
        Spawns the given future on the current executor and returns its [`Task`].

        ### Panics

        Panics if called outside of a [`Runtime`].

        ### Example usage

        ```rust
        use async_io::block_on;

        use mlua::prelude::*;
        use mlua_luau_runtime::*;

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
            rt.spawn_thread(lua.load("spawnBackgroundTask()"), ());
            block_on(rt.run());

            Ok(())
        }
        ```

        [`Runtime`]: crate::Runtime
    */
    fn spawn<T: Send + 'static>(&self, fut: impl Future<Output = T> + Send + 'static) -> Task<T>;
}

impl<'lua> LuaSpawnExt<'lua> for Lua {
    fn spawn<T: Send + 'static>(&self, fut: impl Future<Output = T> + Send + 'static) -> Task<T> {
        let exec = self
            .app_data_ref::<Weak<Executor>>()
            .expect("futures can only be spawned within a runtime")
            .upgrade()
            .expect("executor was dropped");
        exec.spawn(fut)
    }
}
