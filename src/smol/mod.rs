use std::{
    rc::Rc,
    time::{Duration, Instant},
};

use mlua::prelude::*;
use smol::*;

const NUM_TEST_BATCHES: usize = 20;
const NUM_TEST_THREADS: usize = 50_000;

const MAIN_CHUNK: &str = r#"
wait(0.01 * math.random())
"#;

mod executor;
use executor::*;

pub fn main() -> LuaResult<()> {
    let lua = Rc::new(Lua::new());
    let rt = LuaExecutor::new(Rc::clone(&lua));

    lua.globals().set(
        "wait",
        lua.create_async_function(|_, duration: f64| async move {
            let before = Instant::now();
            let after = Timer::after(Duration::from_secs_f64(duration)).await;
            Ok((after - before).as_secs_f64())
        })?,
    )?;

    let start = Instant::now();
    let main_fn = lua.load(MAIN_CHUNK).into_function()?;

    for _ in 0..NUM_TEST_BATCHES {
        rt.run(|lua, lua_exec, _| {
            // TODO: Figure out how to create a scheduler queue that we can
            // append threads to, both front and back, and resume them in order

            for _ in 0..NUM_TEST_THREADS {
                let thread = lua.create_thread(main_fn.clone())?;
                let task = lua_exec.spawn(async move {
                    if let Err(err) = thread.into_async::<_, ()>(()).await {
                        println!("error: {}", err);
                    }
                    Ok::<_, LuaError>(())
                });
                task.detach();
            }

            Ok(())
        })?;
    }

    println!("elapsed: {:?}", start.elapsed());

    Ok(())
}
