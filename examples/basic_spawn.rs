use std::io::ErrorKind;

use async_fs::read_to_string;
use async_io::block_on;

use mlua::prelude::*;
use mlua_luau_runtime::*;

const MAIN_SCRIPT: &str = include_str!("./lua/basic_spawn.luau");

pub fn main() -> LuaResult<()> {
    // Set up persistent Lua environment
    let lua = Lua::new();
    lua.globals().set(
        "readFile",
        lua.create_async_function(|lua, path: String| async move {
            // Spawn background task that does not take up resources on the Lua thread
            let task = lua.spawn(async move {
                match read_to_string(path).await {
                    Ok(s) => Ok(Some(s)),
                    Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
                    Err(e) => Err(e),
                }
            });
            task.await.into_lua_err()
        })?,
    )?;

    // Load the main script into a runtime
    let rt = Runtime::new(&lua)?;
    let main = lua.load(MAIN_SCRIPT);
    rt.spawn_thread(main, ())?;

    // Run until completion
    block_on(rt.run());

    Ok(())
}

#[test]
fn test_basic_spawn() -> LuaResult<()> {
    main()
}
