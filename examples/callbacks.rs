#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

use mlua::prelude::*;
use mlua_luau_runtime::Runtime;

use async_io::block_on;

const MAIN_SCRIPT: &str = include_str!("./lua/callbacks.luau");

pub fn main() -> LuaResult<()> {
    tracing_subscriber::fmt::init();

    // Set up persistent Lua environment
    let lua = Lua::new();

    // Create a new runtime with custom callbacks
    let rt = Runtime::new(&lua);
    rt.set_error_callback(|e| {
        println!(
            "Captured error from Lua!\n{}\n{e}\n{}",
            "-".repeat(15),
            "-".repeat(15)
        );
    });

    // Load the main script into the runtime, and keep track of the thread we spawn
    let main = lua.load(MAIN_SCRIPT);
    let handle = rt.push_thread_front(main, ())?;

    // Run until completion
    block_on(rt.run());

    // We should have gotten the error back from our script
    assert!(handle.result(&lua).unwrap().is_err());

    Ok(())
}

#[test]
fn test_callbacks() -> LuaResult<()> {
    main()
}
