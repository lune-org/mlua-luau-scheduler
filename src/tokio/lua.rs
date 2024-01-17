use mlua::prelude::*;

use crate::tokio::{Message, MessageSender, ThreadId};

pub fn create_lua(lua_tx: MessageSender, async_tx: MessageSender) -> LuaResult<Lua> {
    let lua = Lua::new();
    lua.enable_jit(true);
    lua.set_app_data(async_tx.clone());

    // Cancellation
    let cancel_tx = lua_tx.clone();
    lua.globals().set(
        "__scheduler__cancel",
        LuaFunction::wrap(move |_, thread: LuaThread| {
            let thread_id = ThreadId::from(thread);
            cancel_tx.send(Message::Cancel(thread_id)).into_lua_err()
        }),
    )?;

    // Stdout
    let stdout_tx = async_tx.clone();
    lua.globals().set(
        "__scheduler__writeStdout",
        LuaFunction::wrap(move |_, s: LuaString| {
            let bytes = s.as_bytes().to_vec();
            stdout_tx.send(Message::WriteStdout(bytes)).into_lua_err()
        }),
    )?;

    // Stderr
    let stderr_tx = async_tx.clone();
    lua.globals().set(
        "__scheduler__writeStderr",
        LuaFunction::wrap(move |_, s: LuaString| {
            let bytes = s.as_bytes().to_vec();
            stderr_tx.send(Message::WriteStderr(bytes)).into_lua_err()
        }),
    )?;

    Ok(lua)
}
