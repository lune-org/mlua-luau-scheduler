use mlua::prelude::*;
use mlua_luau_runtime::*;

use async_io::block_on;

const MAIN_SCRIPT: &str = include_str!("./lua/callbacks.luau");

pub fn main() -> LuaResult<()> {
    // Set up persistent Lua environment
    let lua = Lua::new();

    // Create a new runtime with custom callbacks
    let rt = Runtime::new(&lua)?;
    rt.set_error_callback(|e| {
        println!(
            "Captured error from Lua!\n{}\n{e}\n{}",
            "-".repeat(15),
            "-".repeat(15)
        );
    });

    // Load the main script into a runtime
    let main = lua.load(MAIN_SCRIPT);
    rt.spawn_thread(main, ())?;

    // Run until completion
    block_on(rt.run());

    Ok(())
}

#[test]
fn test_callbacks() -> LuaResult<()> {
    main()
}
