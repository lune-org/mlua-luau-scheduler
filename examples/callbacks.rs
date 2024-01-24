use mlua::prelude::*;
use smol_mlua::Runtime;

const MAIN_SCRIPT: &str = include_str!("./lua/callbacks.luau");

pub fn main() -> LuaResult<()> {
    // Set up persistent lua environment
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

    // Load and run the main script until completion
    let main = lua.load(MAIN_SCRIPT);
    rt.spawn_thread(main, ())?;
    rt.run_blocking();

    Ok(())
}

#[test]
fn test_callbacks() -> LuaResult<()> {
    main()
}
