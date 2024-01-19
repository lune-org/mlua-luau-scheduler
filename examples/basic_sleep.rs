use std::time::{Duration, Instant};

use smol_mlua::{
    mlua::prelude::{Lua, LuaResult},
    smol::Timer,
    Runtime,
};

const MAIN_SCRIPT: &str = include_str!("./lua/basic_sleep.luau");

pub fn main() -> LuaResult<()> {
    // Set up persistent lua environment
    let lua = Lua::new();
    lua.globals().set(
        "sleep",
        lua.create_async_function(|_, duration: f64| async move {
            let before = Instant::now();
            let after = Timer::after(Duration::from_secs_f64(duration)).await;
            Ok((after - before).as_secs_f64())
        })?,
    )?;

    // Load the main script into a runtime and run it until completion
    let rt = Runtime::new(&lua)?;
    let main = lua.load(MAIN_SCRIPT);
    rt.push_thread(&lua, main, ());
    rt.run_blocking(&lua);

    Ok(())
}
