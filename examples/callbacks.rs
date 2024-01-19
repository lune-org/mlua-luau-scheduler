use smol_mlua::{
    mlua::prelude::{Lua, LuaResult},
    Callbacks, Runtime,
};

const MAIN_SCRIPT: &str = include_str!("./callbacks.luau");

pub fn main() -> LuaResult<()> {
    // Set up persistent lua environment
    let lua = Lua::new();

    // Load the main script into a runtime
    let rt = Runtime::new(&lua)?;
    let main = lua.load(MAIN_SCRIPT);

    // Inject default value & error callbacks - this will print lua errors to stderr
    Callbacks::default().inject(&lua);

    // Run the main script until completion
    rt.push_main(&lua, main, ());
    rt.run_blocking(&lua);

    Ok(())
}
