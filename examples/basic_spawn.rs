use mlua::ExternalResult;
use smol::io;
use smol_mlua::{
    mlua::prelude::{Lua, LuaResult},
    smol::fs::read_to_string,
    LuaExecutorExt, Runtime,
};

const MAIN_SCRIPT: &str = include_str!("./basic_spawn.luau");

pub fn main() -> LuaResult<()> {
    // Set up persistent lua environment
    let lua = Lua::new();
    lua.globals().set(
        "readFile",
        lua.create_async_function(|lua, path: String| async move {
            // Spawn background task that does not take up resources on the lua thread
            let task = lua.spawn(async move {
                match read_to_string(path).await {
                    Ok(s) => Ok(Some(s)),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
                    Err(e) => Err(e),
                }
            });
            task.await.into_lua_err()
        })?,
    )?;

    // Load the main script into a runtime and run it until completion
    let rt = Runtime::new(&lua)?;
    let main = lua.load(MAIN_SCRIPT);
    rt.push_main(&lua, main, ());
    rt.run_blocking(&lua);

    Ok(())
}
