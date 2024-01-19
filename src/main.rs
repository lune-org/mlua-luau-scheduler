use std::time::{Duration, Instant};

use mlua::prelude::*;
use smol::*;

const MAIN_SCRIPT: &str = include_str!("./main.luau");

use smol_mlua::{thread_callbacks::ThreadCallbacks, thread_runtime::ThreadRuntime};

pub fn main() -> LuaResult<()> {
    let start = Instant::now();
    let lua = Lua::new();

    // Set up persistent lua environment
    lua.globals().set(
        "wait",
        lua.create_async_function(|_, duration: Option<f64>| async move {
            let duration = duration.unwrap_or_default().max(1.0 / 250.0);
            let before = Instant::now();
            let after = Timer::after(Duration::from_secs_f64(duration)).await;
            Ok((after - before).as_secs_f64())
        })?,
    )?;

    // Set up runtime (thread queue / async executors)
    let rt = ThreadRuntime::new(&lua)?;
    let main = rt.push_main(&lua, lua.load(MAIN_SCRIPT), ());
    lua.set_named_registry_value("main", main)?;

    // Add callbacks to capture resulting value/error of main thread
    ThreadCallbacks::new()
        .on_value(|lua, thread, val| {
            let main = lua.named_registry_value::<LuaThread>("main").unwrap();
            if main == thread {
                println!("main thread value: {:?}", val);
            }
        })
        .on_error(|lua, thread, err| {
            let main = lua.named_registry_value::<LuaThread>("main").unwrap();
            if main == thread {
                eprintln!("main thread error: {:?}", err);
            }
        })
        .inject(&lua);

    // Run until end
    rt.run_blocking(&lua);

    println!("elapsed: {:?}", start.elapsed());

    Ok(())
}
