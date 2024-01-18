use std::time::{Duration, Instant};

use mlua::prelude::*;
use smol::*;

const MAIN_CHUNK: &str = r#"
for i = 1, 5 do
    print("iteration " .. tostring(i) .. " of 5")
    local thread = coroutine.running()
    local counter = 0
    for j = 1, 10_000 do
        __scheduler__spawn(function()
            wait(0.1 * math.random())
            counter += 1
            if counter == 10_000 then
                print("completed iteration " .. tostring(i) .. " of 5")
            end
        end)
    end
    coroutine.yield() -- FIXME: This resumes instantly with mlua "async" feature
end
"#;

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
    rt.push_main(&lua, lua.load(MAIN_CHUNK), ());
    rt.run_blocking(&lua);

    println!("elapsed: {:?}", start.elapsed());

    Ok(())
}
