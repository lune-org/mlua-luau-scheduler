use std::time::{Duration, Instant};

use mlua::prelude::*;
use smol::*;

const MAIN_SCRIPT: &str = include_str!("./main.luau");

mod thread_runtime;
mod thread_storage;
mod thread_util;

use thread_runtime::*;
use thread_storage::*;

pub fn main() -> LuaResult<()> {
    let start = Instant::now();
    let lua = Lua::new();

    // Set up persistent lua environment
    lua.globals().set(
        "wait",
        lua.create_async_function(|_, duration: f64| async move {
            let before = Instant::now();
            let after = Timer::after(Duration::from_secs_f64(duration)).await;
            Ok((after - before).as_secs_f64())
        })?,
    )?;

    // Set up runtime (thread queue / async executors) and run main script until end
    let rt = ThreadRuntime::new(&lua)?;
    rt.push_main(&lua, lua.load(MAIN_SCRIPT), ());
    rt.run_blocking(&lua);

    println!("elapsed: {:?}", start.elapsed());

    Ok(())
}
