#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

use async_io::block_on;

use mlua::prelude::*;
use mlua_luau_runtime::{Functions, Runtime};

const MAIN_SCRIPT: &str = include_str!("./lua/exit_code.luau");

pub fn main() -> LuaResult<()> {
    tracing_subscriber::fmt::init();

    // Set up persistent Lua environment
    let lua = Lua::new();
    let rt = Runtime::new(&lua);
    let fns = Functions::new(&lua)?;

    lua.globals().set("exit", fns.exit)?;

    // Load the main script into the runtime
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
