use smol_mlua::{
    mlua::prelude::{Lua, LuaResult},
    Callbacks, Runtime,
};

const MAIN_SCRIPT: &str = include_str!("./callbacks.luau");

pub fn main() -> LuaResult<()> {
    // Set up persistent lua environment
    let lua = Lua::new();

    // Create a new runtime with custom callbacks
    let rt = Runtime::new(&lua)?;
    rt.set_callbacks(
        &lua,
        Callbacks::default().on_error(|_, _, e| {
            println!(
                "Captured error from Lua!\n{}\n{e}\n{}",
                "-".repeat(15),
                "-".repeat(15)
            );
        }),
    );

    // Load and run the main script until completion
    let main = lua.load(MAIN_SCRIPT);
    rt.push_thread(&lua, main, ());
    rt.run_blocking(&lua);

    Ok(())
}
