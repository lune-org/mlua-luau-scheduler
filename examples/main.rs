use std::{
    rc::Rc,
    time::{Duration, Instant},
};

use smol_mlua::{
    mlua::prelude::{Lua, LuaResult, LuaThread, LuaValue},
    smol::{lock::Mutex, Timer},
    Callbacks, IntoLuaThread, Runtime,
};

const MAIN_SCRIPT: &str = include_str!("./main.luau");

pub fn main() -> LuaResult<()> {
    // Set up persistent lua environment
    let lua = Lua::new();
    lua.globals().set(
        "wait",
        lua.create_async_function(|_, duration: Option<f64>| async move {
            let duration = duration.unwrap_or_default().max(1.0 / 250.0);
            let before = Instant::now();
            let after = Timer::after(Duration::from_secs_f64(duration)).await;
            Ok((after - before).as_secs_f64())
        })?,
    )?;

    // Load and run the main script a few times for the purposes of this example
    for _ in 0..20 {
        println!("...");
        match run(&lua, lua.load(MAIN_SCRIPT)) {
            Err(e) => eprintln!("Errored:\n{e}"),
            Ok(v) => println!("Returned value:\n{v:?}"),
        }
    }

    Ok(())
}

/**
    Wrapper function to run the given `main` thread on a new [`Runtime`].

    Waits for all threads to finish, including the main thread, and
    returns the value or error of the main thread once exited.
*/
fn run<'lua>(lua: &'lua Lua, main: impl IntoLuaThread<'lua>) -> LuaResult<LuaValue> {
    // Set up runtime (thread queue / async executors)
    let rt = Runtime::new(lua)?;
    let thread = rt.push_main(lua, main, ());
    lua.set_named_registry_value("mainThread", thread)?;

    // Add callbacks to capture resulting value/error of main thread,
    // we need to do some tricks to get around lifetime issues with 'lua
    // being different inside the callback vs outside the callback for LuaValue
    let captured_error = Rc::new(Mutex::new(None));
    let captured_error_inner = Rc::clone(&captured_error);
    Callbacks::new()
        .on_value(|lua, thread, val| {
            let main: LuaThread = lua.named_registry_value("mainThread").unwrap();
            if main == thread {
                lua.set_named_registry_value("mainValue", val).unwrap();
            }
        })
        .on_error(move |lua, thread, err| {
            let main: LuaThread = lua.named_registry_value("mainThread").unwrap();
            if main == thread {
                captured_error_inner.lock_blocking().replace(err);
            }
        })
        .inject(lua);

    // Run until end
    rt.run_blocking(lua);

    // Extract value and error from their containers
    let err_opt = { captured_error.lock_blocking().take() };
    let val_opt = lua.named_registry_value("mainValue").ok();

    // Check result
    if let Some(err) = err_opt {
        Err(err)
    } else if let Some(val) = val_opt {
        Ok(val)
    } else {
        unreachable!("No value or error captured from main thread");
    }
}
