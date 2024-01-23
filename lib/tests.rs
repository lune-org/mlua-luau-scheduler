use std::time::{Duration, Instant};

use mlua::prelude::*;

use smol::{fs::read_to_string, Timer};

use crate::Runtime;

macro_rules! create_tests {
    ($($name:ident: $value:expr,)*) => { $(
        #[test]
        fn $name() -> LuaResult<()> {
            // Read the test script
            let script = std::fs::read_to_string(concat!($value, ".luau"))?;

            // Set up persistent lua environment
            let lua = Lua::new();
            lua.globals().set(
                "sleep",
                lua.create_async_function(|_, duration: Option<f64>| async move {
                    let duration = duration.unwrap_or_default().max(1.0 / 250.0);
                    let before = Instant::now();
                    let after = Timer::after(Duration::from_secs_f64(duration)).await;
                    Ok((after - before).as_secs_f64())
                })?
            )?;
            lua.globals().set(
                "readFile",
                lua.create_async_function(|_, path: String| async move {
                    Ok(read_to_string(path).await?)
                })?
            )?;

            // Load the main script into a runtime and run it until completion
            let rt = Runtime::new(&lua)?;
            let main = lua.load(script);
            rt.push_thread(main, ());
            rt.run_blocking();

            Ok(())
        }
    )* }
}

create_tests! {
    basic_sleep: "examples/lua/basic_sleep",
    basic_spawn: "examples/lua/basic_spawn",
    callbacks: "examples/lua/callbacks",
    captures: "examples/lua/captures",
    lots_of_threads: "examples/lua/lots_of_threads",
    scheduler_ordering: "examples/lua/scheduler_ordering",
}
