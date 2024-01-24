use std::time::Duration;

use mlua::prelude::*;
use smol::Timer;
use smol_mlua::Runtime;

const MAIN_SCRIPT: &str = include_str!("./lua/lots_of_threads.luau");

const ONE_NANOSECOND: Duration = Duration::from_nanos(1);

pub fn main() -> LuaResult<()> {
    // Set up persistent lua environment, note that we enable thread reuse for
    // mlua's internal async handling since we will be spawning lots of threads
    let lua = Lua::new_with(
        LuaStdLib::ALL,
        LuaOptions::new()
            .catch_rust_panics(false)
            .thread_pool_size(10_000),
    )?;
    let rt = Runtime::new(&lua)?;

    lua.globals().set("spawn", rt.create_spawn_function()?)?;
    lua.globals().set(
        "sleep",
        lua.create_async_function(|_, ()| async move {
            // Obviously we can't sleep for a single nanosecond since
            // this uses OS scheduling under the hood, but we can try
            Timer::after(ONE_NANOSECOND).await;
            Ok(())
        })?,
    )?;

    // Load the main script into the runtime and run it until completion
    let main = lua.load(MAIN_SCRIPT);
    rt.spawn_thread(main, ())?;
    rt.run_blocking();

    Ok(())
}

#[test]
fn test_lots_of_threads() -> LuaResult<()> {
    main()
}
