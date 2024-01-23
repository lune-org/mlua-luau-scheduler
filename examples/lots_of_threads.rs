use std::time::Duration;

use smol_mlua::{
    mlua::prelude::{Lua, LuaResult},
    smol::Timer,
    Runtime,
};

const MAIN_SCRIPT: &str = include_str!("./lua/lots_of_threads.luau");

const ONE_NANOSECOND: Duration = Duration::from_nanos(1);

pub fn main() -> LuaResult<()> {
    // Set up persistent lua environment
    let lua = Lua::new();
    lua.globals().set(
        "sleep",
        lua.create_async_function(|_, ()| async move {
            // Obviously we can't sleep for a single nanosecond since
            // this uses OS scheduling under the hood, but we can try
            Timer::after(ONE_NANOSECOND).await;
            Ok(())
        })?,
    )?;

    // Load the main script into a runtime and run it until completion
    let rt = Runtime::new(&lua)?;
    let main = lua.load(MAIN_SCRIPT);
    rt.push_thread(main, ());
    rt.run_blocking();

    Ok(())
}

#[test]
fn test_lots_of_threads() -> LuaResult<()> {
    main()
}
