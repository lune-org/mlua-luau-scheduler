use std::time::Duration;

use mlua::prelude::*;
use tokio::time::Instant;

use crate::{Message, MessageSender, ThreadId};

pub fn create_lua(tx: MessageSender) -> LuaResult<Lua> {
    let lua = Lua::new();
    lua.enable_jit(true);
    lua.set_app_data(tx.clone());

    // Resumption
    let tx_resume = tx.clone();
    lua.globals().set(
        "__scheduler__resumeAfter",
        LuaFunction::wrap(move |lua, duration: f64| {
            let thread_id = ThreadId::from(lua.current_thread());
            let yielded_at = Instant::now();
            let duration = Duration::from_secs_f64(duration);
            tx_resume
                .send(Message::Sleep(thread_id, yielded_at, duration))
                .into_lua_err()
        }),
    )?;

    // Cancellation
    let tx_cancel = tx.clone();
    lua.globals().set(
        "__scheduler__cancel",
        LuaFunction::wrap(move |_, thread: LuaThread| {
            let thread_id = ThreadId::from(thread);
            tx_cancel.send(Message::Cancel(thread_id)).into_lua_err()
        }),
    )?;

    // Stdout
    let tx_stdout = tx.clone();
    lua.globals().set(
        "__scheduler__writeStdout",
        LuaFunction::wrap(move |_, s: LuaString| {
            let bytes = s.as_bytes().to_vec();
            tx_stdout.send(Message::WriteStdout(bytes)).into_lua_err()
        }),
    )?;

    // Stderr
    let tx_stderr = tx.clone();
    lua.globals().set(
        "__scheduler__writeStderr",
        LuaFunction::wrap(move |_, s: LuaString| {
            let bytes = s.as_bytes().to_vec();
            tx_stderr.send(Message::WriteStderr(bytes)).into_lua_err()
        }),
    )?;

    Ok(lua)
}
