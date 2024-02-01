#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

use std::process::ExitCode;

use async_io::block_on;

use mlua::prelude::*;
use mlua_luau_runtime::{LuaRuntimeExt, Runtime};

const MAIN_SCRIPT: &str = include_str!("./lua/exit_code.luau");

const EXIT_IMPL_LUA: &str = r"
exit(...)
yield()
";

pub fn main() -> LuaResult<()> {
    tracing_subscriber::fmt::init();

    // Set up persistent Lua environment
    let lua = Lua::new();

    // Note that our exit function is partially implemented in Lua
    // because we need to also yield the thread that called it, this
    // is not possible to do in Rust because of crossing C-call boundary
    let exit_fn_env = lua.create_table_from(vec![
        (
            "exit",
            lua.create_function(|lua, code: Option<u8>| {
                let code = code.map(ExitCode::from).unwrap_or_default();
                lua.set_exit_code(code);
                Ok(())
            })?,
        ),
        (
            "yield",
            lua.globals()
                .get::<_, LuaTable>("coroutine")?
                .get::<_, LuaFunction>("yield")?,
        ),
    ])?;

    let exit_fn = lua
        .load(EXIT_IMPL_LUA)
        .set_environment(exit_fn_env)
        .into_function()?;
    lua.globals().set("exit", exit_fn)?;

    // Load the main script into a runtime
    let rt = Runtime::new(&lua);
    let main = lua.load(MAIN_SCRIPT);
    rt.push_thread_front(main, ())?;

    // Run until completion
    block_on(rt.run());

    // Verify that we got a correct exit code
    let code = rt.get_exit_code().unwrap_or_default();
    assert!(format!("{code:?}").contains("(1)"));

    Ok(())
}

#[test]
fn test_exit_code() -> LuaResult<()> {
    main()
}
